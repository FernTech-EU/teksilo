// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Spatial index for [`Scene`](crate::Scene) items.
//!
//! [`GridHashIndex`] is the only shipped implementation — a uniform grid
//! hash. The [`SpatialIndex`] trait is deliberately small — three mutating
//! operations (`insert`, `remove`, `query`) plus two read methods
//! (`contains`, `len`) — so an application that needs different behaviour
//! (e.g. an R-tree) can supply its own implementation in a one-line change
//! via [`Scene::with_index`](crate::Scene::with_index).
//!
//! ## Why grid hash first
//!
//! - Cache-friendly: items in the same cell are stored contiguously.
//! - Insert / remove / move are amortised `O(k)` where `k` is the
//!   number of cells the item overlaps (typically 1–4 for items
//!   smaller than the cell size).
//! - `query(rect)` returns deduplicated candidates from the cells the
//!   rect overlaps; callers can narrow with a per-item AABB check.
//! - Pathological case (one giant item that spans hundreds of cells)
//!   is rare and easily worked around by raising `cell_size` for
//!   that scene. A custom `SpatialIndex` would handle non-uniform
//!   density better — an R-tree, say, for an editor with many
//!   overlapping items — but none ships; the trait is the place to
//!   add one.
//!
//! Default `cell_size` is [`DEFAULT_CELL_SIZE`] (`256.0` logical pixels)
//! — large enough that typical card-sized items (~200 px) bucket into 1–4
//! cells and small enough that viewport queries (~800–1200 px) hit a
//! manageable fan-out.
//!
//! ## Example
//!
//! ```ignore
//! // ItemId values are obtained from Scene::add_item in real code;
//! // the example uses the crate-internal constructor for illustration.
//! use bastyde_scene::{GridHashIndex, SpatialIndex, ItemId};
//! use bastyde_canvas::Rect;
//!
//! let mut index = GridHashIndex::default();
//! let id = ItemId(1); // in practice: returned by Scene::add_item
//! index.insert(id, Rect::new(10.0, 10.0, 80.0, 80.0));
//! assert!(index.contains(id));
//!
//! let hits = index.query(Rect::new(0.0, 0.0, 100.0, 100.0));
//! assert!(hits.contains(&id));
//!
//! index.remove(id);
//! assert!(index.is_empty());
//! ```

use std::collections::{HashMap, HashSet};

use bastyde_canvas::Rect;

use crate::item::ItemId;

/// A spatial index over [`ItemId`]s keyed by axis-aligned scene
/// rectangles. Used by [`Scene`](crate::Scene) for `items_in_rect`
/// queries and by [`SceneView`](crate::SceneView) for viewport
/// culling.
pub trait SpatialIndex: Send + std::fmt::Debug {
    /// Insert or update an item's bounds. Calling `insert` again with
    /// the same id replaces the previous bounds (re-buckets the
    /// item). Equivalent to `remove(id); insert(id, bounds);` on
    /// implementations that need an explicit update path.
    fn insert(&mut self, id: ItemId, bounds: Rect);

    /// Remove an item. No-op if `id` is not present.
    fn remove(&mut self, id: ItemId);

    /// Items whose bounds intersect `scene_rect`, in implementation-
    /// defined order. The result is deduplicated. May include false
    /// positives (items in cells the rect overlaps but whose bounds
    /// don't actually intersect) — callers that need exact
    /// intersection narrow with a per-item check.
    fn query(&self, scene_rect: Rect) -> Vec<ItemId>;

    /// Whether `id` is currently in the index.
    fn contains(&self, id: ItemId) -> bool;

    /// Total number of items in the index.
    fn len(&self) -> usize;

    /// Whether the index is empty.
    fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

/// Default cell size for [`GridHashIndex`] — 256 logical pixels.
/// Item-side typical 200 px cards bucket into 1–4 cells; viewport
/// queries (~800–1200 px) hit a small fan-out.
pub const DEFAULT_CELL_SIZE: f32 = 256.0;

/// Uniform grid spatial hash. Each item is bucketed into every cell
/// its AABB overlaps; queries union all items from the cells the
/// query rect overlaps.
#[derive(Debug)]
pub struct GridHashIndex {
    cell_size: f32,
    cells: HashMap<(i32, i32), Vec<ItemId>>,
    /// Reverse lookup so `remove` and `insert` (as update) don't need
    /// to scan every cell.
    item_cells: HashMap<ItemId, Vec<(i32, i32)>>,
}

impl GridHashIndex {
    /// Create a grid with `cell_size` logical pixels per cell.
    /// Clamped to a minimum of 1.0 to avoid pathological huge bucket
    /// counts.
    pub fn new(cell_size: f32) -> Self {
        Self {
            cell_size: cell_size.max(1.0),
            cells: HashMap::new(),
            item_cells: HashMap::new(),
        }
    }

    /// The configured cell size in logical pixels.
    pub fn cell_size(&self) -> f32 {
        self.cell_size
    }

    /// Number of cells currently storing at least one item. Useful
    /// for diagnostics; not part of the public `SpatialIndex` trait.
    pub fn cell_count(&self) -> usize {
        self.cells.len()
    }

    /// Test-only staleness probe: `true` if any bucket in `cells` is
    /// present but empty. `remove` is supposed to drop a cell entry
    /// entirely once its last item leaves (see the `items.is_empty()`
    /// check there) — an empty-but-present bucket is a leak that would
    /// otherwise only show up as `cell_count()` drifting upward over a
    /// long-lived scene. Not part of the public API; added for the
    /// proptest suite below rather than making `cells` `pub`.
    #[cfg(test)]
    fn has_empty_bucket(&self) -> bool {
        self.cells.values().any(|items| items.is_empty())
    }

    fn cells_for_rect(&self, r: Rect) -> Vec<(i32, i32)> {
        // Half-open convention: a rect that ends exactly on a cell
        // boundary doesn't include the next cell. Otherwise an item
        // sitting on a boundary would over-bucket and queries would
        // double-count.
        let cs = self.cell_size;
        let min_x = (r.x / cs).floor() as i32;
        let min_y = (r.y / cs).floor() as i32;
        // For zero-extent rects, treat as a single point cell.
        let max_x = if r.width <= 0.0 {
            min_x
        } else {
            ((r.right() - f32::EPSILON) / cs).floor() as i32
        };
        let max_y = if r.height <= 0.0 {
            min_y
        } else {
            ((r.bottom() - f32::EPSILON) / cs).floor() as i32
        };
        let mut out =
            Vec::with_capacity(((max_x - min_x + 1).max(1) * (max_y - min_y + 1).max(1)) as usize);
        for x in min_x..=max_x {
            for y in min_y..=max_y {
                out.push((x, y));
            }
        }
        out
    }
}

impl Default for GridHashIndex {
    fn default() -> Self {
        Self::new(DEFAULT_CELL_SIZE)
    }
}

impl SpatialIndex for GridHashIndex {
    fn insert(&mut self, id: ItemId, bounds: Rect) {
        // Re-insert: drop old buckets first.
        self.remove(id);
        let cells = self.cells_for_rect(bounds);
        for cell in &cells {
            self.cells.entry(*cell).or_default().push(id);
        }
        self.item_cells.insert(id, cells);
    }

    fn remove(&mut self, id: ItemId) {
        if let Some(cells) = self.item_cells.remove(&id) {
            for cell in cells {
                if let Some(items) = self.cells.get_mut(&cell) {
                    items.retain(|&i| i != id);
                    if items.is_empty() {
                        self.cells.remove(&cell);
                    }
                }
            }
        }
    }

    fn query(&self, scene_rect: Rect) -> Vec<ItemId> {
        let mut seen = HashSet::new();
        let mut result = Vec::new();
        for cell in self.cells_for_rect(scene_rect) {
            if let Some(items) = self.cells.get(&cell) {
                for &id in items {
                    if seen.insert(id) {
                        result.push(id);
                    }
                }
            }
        }
        // Stable order so query results are deterministic across
        // runs — useful for tests and reproducible debugging.
        result.sort_unstable();
        result
    }

    fn contains(&self, id: ItemId) -> bool {
        self.item_cells.contains_key(&id)
    }

    fn len(&self) -> usize {
        self.item_cells.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn id(n: u64) -> ItemId {
        // Build a synthetic ItemId for tests. Tests only care about
        // uniqueness of the underlying integer, not collision-freedom
        // against the global counter — the index is keyed by ItemId
        // equality which is just the inner u64.
        ItemId(n)
    }

    #[test]
    fn insert_and_contains() {
        let mut g = GridHashIndex::default();
        g.insert(id(1), Rect::new(10.0, 10.0, 50.0, 50.0));
        assert!(g.contains(id(1)));
        assert_eq!(g.len(), 1);
    }

    #[test]
    fn remove_removes_from_all_cells() {
        let mut g = GridHashIndex::new(64.0);
        // 200×200 rect spans multiple cells.
        g.insert(id(1), Rect::new(0.0, 0.0, 200.0, 200.0));
        assert!(g.contains(id(1)));
        let cells_with_id = g.cell_count();
        assert!(cells_with_id > 1, "expected multi-cell bucketing");
        g.remove(id(1));
        assert!(!g.contains(id(1)));
        assert_eq!(g.len(), 0);
        assert_eq!(
            g.cell_count(),
            0,
            "all buckets should drop with the only item"
        );
    }

    #[test]
    fn re_insert_replaces_buckets() {
        // Inserting the same id twice with different bounds must
        // re-bucket — a stale entry would cause query() to return
        // hits for the old bounds.
        let mut g = GridHashIndex::new(64.0);
        g.insert(id(1), Rect::new(0.0, 0.0, 50.0, 50.0));
        g.insert(id(1), Rect::new(1000.0, 1000.0, 50.0, 50.0));
        assert!(g.query(Rect::new(0.0, 0.0, 100.0, 100.0)).is_empty());
        let hits = g.query(Rect::new(990.0, 990.0, 100.0, 100.0));
        assert_eq!(hits, vec![id(1)]);
    }

    #[test]
    fn query_returns_intersecting_items() {
        let mut g = GridHashIndex::new(64.0);
        g.insert(id(1), Rect::new(0.0, 0.0, 50.0, 50.0));
        g.insert(id(2), Rect::new(200.0, 0.0, 50.0, 50.0));
        g.insert(id(3), Rect::new(0.0, 200.0, 50.0, 50.0));

        let near_origin = g.query(Rect::new(0.0, 0.0, 100.0, 100.0));
        assert_eq!(near_origin, vec![id(1)]);

        let far_right = g.query(Rect::new(180.0, 0.0, 100.0, 100.0));
        assert_eq!(far_right, vec![id(2)]);

        let nothing = g.query(Rect::new(500.0, 500.0, 1.0, 1.0));
        assert!(nothing.is_empty());
    }

    #[test]
    fn query_dedupes_items_spanning_multiple_cells() {
        // An item that spans multiple cells must appear once per
        // query, not once per overlapped cell.
        let mut g = GridHashIndex::new(50.0);
        g.insert(id(1), Rect::new(0.0, 0.0, 200.0, 200.0));
        let hits = g.query(Rect::new(0.0, 0.0, 200.0, 200.0));
        assert_eq!(hits, vec![id(1)]);
    }

    #[test]
    fn query_matches_brute_force_on_random_layout() {
        // Cross-check the index against a brute-force AABB intersect
        // scan over a deterministic random layout. Pins both the
        // bucketing math and the query path.
        use std::collections::BTreeSet;
        let mut g = GridHashIndex::new(64.0);
        let mut items: Vec<(ItemId, Rect)> = Vec::new();
        // Deterministic LCG so the test is reproducible without a
        // RNG dep.
        let mut state: u64 = 0xDEAD_BEEF;
        let mut next = || {
            state = state
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407);
            (state >> 33) as u32
        };
        for n in 0..200 {
            let x = (next() % 1000) as f32 - 500.0;
            let y = (next() % 1000) as f32 - 500.0;
            let w = (next() % 80) as f32 + 5.0;
            let h = (next() % 80) as f32 + 5.0;
            let r = Rect::new(x, y, w, h);
            let id = ItemId(n + 1);
            items.push((id, r));
            g.insert(id, r);
        }

        let queries = [
            Rect::new(-100.0, -100.0, 200.0, 200.0),
            Rect::new(0.0, 0.0, 50.0, 50.0),
            Rect::new(-500.0, -500.0, 1000.0, 1000.0),
            Rect::new(100.0, 100.0, 30.0, 30.0),
            Rect::new(2000.0, 2000.0, 10.0, 10.0),
        ];
        for q in queries {
            let from_index: BTreeSet<ItemId> = g.query(q).into_iter().collect();
            let from_brute: BTreeSet<ItemId> = items
                .iter()
                .filter(|(_, r)| rects_intersect(*r, q))
                .map(|(id, _)| *id)
                .collect();

            // The trait contract allows the index to return cell
            // fan-out false positives — items whose cell overlaps
            // the query rect but whose AABB doesn't actually
            // intersect. So `from_brute ⊆ from_index`, and after
            // narrowing the index hits with the same intersect
            // predicate the result must equal the brute-force set.
            assert!(
                from_brute.is_subset(&from_index),
                "index missed true intersections for query {:?}: missing {:?}",
                q,
                from_brute.difference(&from_index).collect::<Vec<_>>()
            );
            let narrowed: BTreeSet<ItemId> = from_index
                .iter()
                .copied()
                .filter(|id| {
                    let r = items
                        .iter()
                        .find(|(i, _)| i == id)
                        .unwrap_or_else(|| {
                            panic!(
                                "query returned id {id:?} not present in items — \
                                 phantom id from GridHashIndex"
                            )
                        })
                        .1;
                    rects_intersect(r, q)
                })
                .collect();
            assert_eq!(
                narrowed, from_brute,
                "narrowed index disagrees with brute-force for query {:?}",
                q
            );
        }
    }

    #[test]
    fn cell_size_clamped_to_minimum_one() {
        let g = GridHashIndex::new(0.0);
        assert!(g.cell_size() >= 1.0);
        let g = GridHashIndex::new(-100.0);
        assert!(g.cell_size() >= 1.0);
    }

    #[test]
    fn perf_microbench_insert_query() {
        // Not a strict bound — just a smoke test that 1000 inserts
        // followed by 1000 queries run in sub-millisecond time on
        // any reasonable hardware. If this regresses to seconds,
        // something has gone catastrophically wrong with the
        // bucketing math.
        use std::time::Instant;
        let mut g = GridHashIndex::default();
        let start = Instant::now();
        for n in 0..1000u64 {
            let x = ((n * 37) % 5000) as f32;
            let y = ((n * 53) % 5000) as f32;
            g.insert(ItemId(n + 1), Rect::new(x, y, 40.0, 40.0));
        }
        let insert_ms = start.elapsed().as_millis();
        let start = Instant::now();
        let mut total = 0;
        for n in 0..1000u64 {
            let x = ((n * 7) % 5000) as f32;
            let y = ((n * 11) % 5000) as f32;
            total += g.query(Rect::new(x, y, 100.0, 100.0)).len();
        }
        let query_ms = start.elapsed().as_millis();
        // Loose bound (debug builds): both should easily finish
        // under 100 ms each on any developer laptop.
        assert!(
            insert_ms < 200,
            "1000 inserts took {} ms — perf regression?",
            insert_ms
        );
        assert!(
            query_ms < 200,
            "1000 queries took {} ms — perf regression?",
            query_ms
        );
        // Sanity: queries actually returned something.
        assert!(total > 0);
    }

    fn rects_intersect(a: Rect, b: Rect) -> bool {
        a.x < b.x + b.width && b.x < a.x + a.width && a.y < b.y + b.height && b.y < a.y + a.height
    }
}

/// Property-based tests for [`GridHashIndex`].
///
/// `GridHashIndex` is `pub(crate)` (see `pub(crate) mod index;` in
/// `lib.rs`), so a `tests/*.rs` integration file cannot reach it —
/// this suite lives inline, after the example-based `mod tests` above,
/// per house style.
///
/// The central risk here is coordinate/cell arithmetic: `cells_for_rect`
/// divides by `cell_size` and floors, so an item and a query rect that
/// truly intersect could in principle land in disjoint cell sets if
/// `f32` rounding shifts a boundary coordinate by even one ULP at large
/// magnitude — that would be a *missed hit* (`query` returning fewer
/// items than a brute-force scan), which is worse than the documented
/// over-reporting the trait contract explicitly allows. `cargo-fuzz`
/// needs nightly + libfuzzer-sys, which isn't assumed here; proptest
/// with a few hundred to a couple thousand iterations per property
/// (override via `PROPTEST_CASES=N`) gives the "never misses a hit /
/// never panics on weird input" coverage a fuzz corpus would, plus
/// shrinking to a minimal counterexample. Generators are hand-written
/// per-file (no `prop_compose!`/`Arbitrary`), matching the sibling
/// `../text-typeset` / `../text-document` proptest suites.
#[cfg(test)]
mod proptests {
    use std::collections::{BTreeSet, HashMap};

    use proptest::prelude::*;

    use super::*;

    fn id(n: u64) -> ItemId {
        ItemId(n)
    }

    fn rects_intersect(a: Rect, b: Rect) -> bool {
        a.x < b.x + b.width && b.x < a.x + a.width && a.y < b.y + b.height && b.y < a.y + a.height
    }

    // The previous (incomplete) investigation swept exactly these cell
    // sizes looking for a boundary/precision bug — keep the same set so
    // this suite actually covers the suspect region instead of picking
    // fresh, possibly-safer values.
    fn arb_cell_size() -> impl Strategy<Value = f32> {
        prop_oneof![
            Just(1.0_f32),
            Just(8.0_f32),
            Just(50.0_f32),
            Just(64.0_f32),
            Just(256.0_f32),
        ]
    }

    // A coordinate generator biased three ways at once: (a) exact
    // multiples of the cell size — sits exactly ON a cell boundary;
    // (b) a multiple plus a tiny nudge — sits just inside/outside a
    // boundary, where `floor` is most sensitive to `f32` rounding;
    // (c) plain large-magnitude values, unrelated to any boundary, at
    // the same magnitudes the prior investigation swept
    // (1 / 100 / 5_000 / 100_000 / 1_000_000, both signs). Negative
    // coordinates are included throughout since the grid has no
    // origin restriction.
    fn arb_coord(cell_size: f32) -> impl Strategy<Value = f32> {
        let cs = cell_size;
        prop_oneof![
            3 => (-2000i32..2000i32).prop_map(move |k| k as f32 * cs),
            3 => (-2000i32..2000i32, -3i32..=3i32)
                .prop_map(move |(k, e)| k as f32 * cs + e as f32 * 1.0e-3),
            2 => prop_oneof![
                Just(1.0_f32),
                Just(-1.0_f32),
                Just(100.0_f32),
                Just(-100.0_f32),
                Just(5_000.0_f32),
                Just(-5_000.0_f32),
                Just(100_000.0_f32),
                Just(-100_000.0_f32),
                Just(1_000_000.0_f32),
                Just(-1_000_000.0_f32),
            ],
            2 => -1_000_000.0f32..1_000_000.0f32,
        ]
    }

    // Zero and near-zero extents are the "single point" edge case
    // documented in `cells_for_rect`'s width/height <= 0.0 branch; we
    // also want ordinary and very large extents.
    /// Cap on how many cells a single generated item may span per axis.
    ///
    /// `GridHashIndex::insert` allocates one `(i32, i32)` per covered cell
    /// (see `cells_for_rect`), so cell count grows as
    /// `(width / cell_size) * (height / cell_size)` with NO upper bound. An
    /// unconstrained extent combined with the smallest `cell_size` this
    /// suite generates (1.0) asks for `1e6 * 1e6 = 1e12` cells and OOMs the
    /// machine before any assertion runs — which is how this suite came to
    /// take a developer's workstation down.
    ///
    /// That unbounded growth is a REAL BUG in `cells_for_rect`, not merely a
    /// bad generator — a scene that adds one very large item (a backdrop, a
    /// full-document canvas rect) with a small `cell_size` will hang or OOM
    /// in production, with no test needed to provoke it. Fixing it is a
    /// design decision (cap coverage and keep an always-scanned "oversized"
    /// list, or clamp to a world bound), so it is reported rather than
    /// patched here. This constant keeps the suite able to explore geometry
    /// safely in the meantime.
    const MAX_CELLS_PER_AXIS: f32 = 64.0;

    /// Extents are generated RELATIVE to `cell_size` so an item never spans
    /// more than `MAX_CELLS_PER_AXIS` cells. Large absolute *coordinates* are
    /// still generated by `arb_coord` — those are the interesting case for
    /// the f32-precision-at-magnitude question, and they are cheap because a
    /// far-away rect is still only a few cells.
    fn arb_extent(cell_size: f32) -> impl Strategy<Value = f32> {
        let cs = cell_size;
        let max = cs * MAX_CELLS_PER_AXIS;
        prop_oneof![
            // Degenerate extents: a zero-size rect must still bucket to
            // exactly one cell, and a sub-pixel one must not round to zero.
            Just(0.0_f32),
            Just(0.001_f32),
            // Sub-cell, exactly one cell, and a few cells — the boundary
            // arithmetic in `cells_for_rect` lives here.
            (0.01f32..1.0f32).prop_map(move |f| f * cs),
            Just(cs),
            (1.0f32..MAX_CELLS_PER_AXIS).prop_map(move |f| f * cs),
            Just(max),
        ]
    }

    /// A `cell_size` paired with two rects generated FOR THAT cell size.
    ///
    /// Drawing the grid's `cell_size` and a rect's extent from two
    /// INDEPENDENT `arb_cell_size()` draws is a trap: a rect sized for a
    /// 256 px grid (extent up to `256 * MAX_CELLS_PER_AXIS`) inserted into a
    /// 1 px grid spans ~268 million cells and allocates gigabytes. Every
    /// property that builds a grid and inserts into it must take its rects
    /// from here so the two stay coupled.
    fn arb_grid_and_two_rects() -> impl Strategy<Value = (f32, Rect, Rect)> {
        arb_cell_size().prop_flat_map(|cs| (Just(cs), arb_rect(cs), arb_rect(cs)))
    }

    fn arb_rect(cell_size: f32) -> impl Strategy<Value = Rect> {
        (
            arb_coord(cell_size),
            arb_coord(cell_size),
            arb_extent(cell_size),
            arb_extent(cell_size),
        )
            .prop_map(|(x, y, width, height)| Rect::new(x, y, width, height))
    }

    // (cell_size, items, query_rect) sharing one cell_size so the
    // boundary-biased coordinates in `items` and `query_rect` actually
    // land relative to the same grid.
    fn arb_layout_and_query() -> impl Strategy<Value = (f32, Vec<(u64, Rect)>, Rect)> {
        arb_cell_size().prop_flat_map(|cs| {
            (
                Just(cs),
                prop::collection::vec((0u64..64, arb_rect(cs)), 0..40),
                arb_rect(cs),
            )
        })
    }

    fn build_index(
        cell_size: f32,
        items: &[(u64, Rect)],
    ) -> (GridHashIndex, HashMap<ItemId, Rect>) {
        let mut g = GridHashIndex::new(cell_size);
        let mut model: HashMap<ItemId, Rect> = HashMap::new();
        for &(n, r) in items {
            let i = id(n);
            g.insert(i, r);
            model.insert(i, r);
        }
        (g, model)
    }

    // ── 1. query, narrowed by a true intersection test, equals a brute-force scan ──
    proptest! {
        #![proptest_config(ProptestConfig { cases: 1024, ..ProptestConfig::default() })]
        #[test]
        fn query_narrowed_matches_brute_force((cell_size, items, q) in arb_layout_and_query()) {
            let (g, model) = build_index(cell_size, &items);

            let from_index: BTreeSet<ItemId> = g.query(q).into_iter().collect();
            let from_brute: BTreeSet<ItemId> = model
                .iter()
                .filter(|(_, r)| rects_intersect(**r, q))
                .map(|(i, _)| *i)
                .collect();

            // The index may over-report (cell fan-out false positives)
            // but must never under-report a true intersection — a
            // missed hit is a lost click.
            prop_assert!(
                from_brute.is_subset(&from_index),
                "cell_size={} query={:?}: index missed true intersections {:?} (model={:?})",
                cell_size,
                q,
                from_brute.difference(&from_index).collect::<Vec<_>>(),
                model,
            );

            let narrowed: BTreeSet<ItemId> = from_index
                .iter()
                .copied()
                .filter(|i| rects_intersect(model[i], q))
                .collect();
            prop_assert_eq!(
                &narrowed, &from_brute,
                "cell_size={} query={:?}: narrowed index result {:?} != brute force {:?}",
                cell_size, q, narrowed, from_brute,
            );
        }
    }

    // ── 2. contains/len track a HashMap<ItemId, Rect> model over insert/remove/re-insert ──
    #[derive(Debug, Clone)]
    enum Op {
        Insert(u64, Rect),
        Remove(u64),
    }

    fn arb_op(cell_size: f32) -> impl Strategy<Value = Op> {
        prop_oneof![
            3 => (0u64..12, arb_rect(cell_size)).prop_map(|(n, r)| Op::Insert(n, r)),
            1 => (0u64..12).prop_map(Op::Remove),
        ]
    }

    proptest! {
        #![proptest_config(ProptestConfig { cases: 512, ..ProptestConfig::default() })]
        #[test]
        fn contains_and_len_track_a_hashmap_model(
            (cell_size, ops) in arb_cell_size()
                .prop_flat_map(|cs| (Just(cs), prop::collection::vec(arb_op(cs), 0..80)))
        ) {
            let mut g = GridHashIndex::new(cell_size);
            let mut model: HashMap<ItemId, Rect> = HashMap::new();

            for op in &ops {
                match *op {
                    Op::Insert(n, r) => {
                        g.insert(id(n), r);
                        model.insert(id(n), r);
                    }
                    Op::Remove(n) => {
                        g.remove(id(n));
                        model.remove(&id(n));
                    }
                }
                prop_assert_eq!(
                    g.len(), model.len(),
                    "after {:?}: index len {} != model len {} (ops so far: {:?})",
                    op, g.len(), model.len(), ops,
                );
            }

            // Full-state check over the bounded id range every op draws
            // from, since there's no "list all ids" accessor.
            for n in 0..12u64 {
                prop_assert_eq!(
                    g.contains(id(n)), model.contains_key(&id(n)),
                    "id {} contains()={} but model has_key={} after ops {:?}",
                    n, g.contains(id(n)), model.contains_key(&id(n)), ops,
                );
            }
        }
    }

    // ── 3. no empty cell buckets linger after any insert/remove sequence ──
    proptest! {
        #![proptest_config(ProptestConfig { cases: 512, ..ProptestConfig::default() })]
        #[test]
        fn no_empty_bucket_lingers_after_removals(
            (cell_size, ops) in arb_cell_size()
                .prop_flat_map(|cs| (Just(cs), prop::collection::vec(arb_op(cs), 0..80)))
        ) {
            let mut g = GridHashIndex::new(cell_size);
            for op in &ops {
                match *op {
                    Op::Insert(n, r) => g.insert(id(n), r),
                    Op::Remove(n) => g.remove(id(n)),
                }
                prop_assert!(
                    !g.has_empty_bucket(),
                    "empty bucket left behind after {:?} (ops so far: {:?})",
                    op, ops,
                );
            }
        }
    }

    // ── 4. re-inserting an existing id under a new rect fully relocates it ──
    proptest! {
        #![proptest_config(ProptestConfig { cases: 512, ..ProptestConfig::default() })]
        #[test]
        fn reinsert_relocates_with_no_ghost_at_old_rect(
            cell_size in arb_cell_size(),
            old_n in 0u64..5,
            new_n in 0u64..5,
        ) {
            // Keep old/new rects far enough apart (relative to cell_size)
            // that they cannot share a cell, so a stale entry at the old
            // location is unambiguously observable via query.
            let old_rect = Rect::new(0.0, 0.0, cell_size.min(50.0), cell_size.min(50.0));
            let far = cell_size * 10_000.0;
            let new_rect = Rect::new(far, far, cell_size.min(50.0), cell_size.min(50.0));

            let mut g = GridHashIndex::new(cell_size);
            g.insert(id(old_n), old_rect);
            g.insert(id(new_n), new_rect);
            // Re-insert new_n's id (which may or may not equal old_n) at
            // the far location again isn't the point here; instead move
            // one id from the old spot to the new spot and check the old
            // spot lost it.
            let moved = id(old_n);
            g.insert(moved, new_rect);

            let at_old = g.query(old_rect);
            prop_assert!(
                !at_old.contains(&moved),
                "ghost entry: {:?} still found at the old rect {:?} after re-insert to {:?}",
                moved, old_rect, new_rect,
            );
            let at_new = g.query(new_rect);
            prop_assert!(
                at_new.contains(&moved),
                "{:?} not found at its new rect {:?} after re-insert",
                moved, new_rect,
            );
        }
    }

    // ── 5. query result set does not depend on insertion order ──
    //
    // The permutation is produced by the decorate-sort-undecorate trick —
    // proptest hands us a `Vec<u32>` of sort keys the same length as the
    // (deduped) item list, we zip+sort by key — rather than a hand-rolled
    // RNG/shuffle: all the randomness comes from the strategy, and
    // shrinking still applies to the keys like any other generated value.
    fn arb_order_independence_case()
    -> impl Strategy<Value = (f32, Vec<(u64, Rect)>, Rect, Vec<u32>)> {
        arb_layout_and_query().prop_flat_map(|(cs, items, q)| {
            // Dedup ids first (later duplicate wins, matching HashMap
            // insert semantics) so both orderings insert the same final
            // id->rect mapping. Done here (inside the strategy) so the
            // shuffle-key vector can be sized to match exactly.
            let mut model: Vec<(u64, Rect)> = Vec::new();
            for &(n, r) in &items {
                if let Some(slot) = model.iter_mut().find(|(mn, _)| *mn == n) {
                    slot.1 = r;
                } else {
                    model.push((n, r));
                }
            }
            let n = model.len();
            (
                Just(cs),
                Just(model),
                Just(q),
                prop::collection::vec(any::<u32>(), n),
            )
        })
    }

    proptest! {
        #![proptest_config(ProptestConfig { cases: 256, ..ProptestConfig::default() })]
        #[test]
        fn query_is_independent_of_insertion_order(
            (cell_size, model, q, keys) in arb_order_independence_case()
        ) {
            let forward = model.clone();
            let mut decorated: Vec<((u64, Rect), u32)> =
                model.iter().copied().zip(keys.iter().copied()).collect();
            decorated.sort_by_key(|(_, k)| *k);
            let shuffled: Vec<(u64, Rect)> = decorated.into_iter().map(|(item, _)| item).collect();

            let mut g_forward = GridHashIndex::new(cell_size);
            for &(n, r) in &forward {
                g_forward.insert(id(n), r);
            }
            let mut g_shuffled = GridHashIndex::new(cell_size);
            for &(n, r) in &shuffled {
                g_shuffled.insert(id(n), r);
            }

            let forward_hits: BTreeSet<ItemId> = g_forward.query(q).into_iter().collect();
            let shuffled_hits: BTreeSet<ItemId> = g_shuffled.query(q).into_iter().collect();
            prop_assert_eq!(
                &forward_hits, &shuffled_hits,
                "insertion order changed query({:?}) result: forward {:?} != shuffled {:?} (items order: {:?} vs {:?})",
                q, forward_hits, shuffled_hits, forward, shuffled,
            );
        }
    }

    // ── 6. inserting the same id with identical bounds twice is a no-op ──
    proptest! {
        #![proptest_config(ProptestConfig { cases: 512, ..ProptestConfig::default() })]
        #[test]
        fn reinserting_identical_bounds_is_idempotent(
            n in 0u64..20,
            (cell_size, r, q) in arb_grid_and_two_rects(),
        ) {
            let mut g = GridHashIndex::new(cell_size);
            g.insert(id(n), r);
            let len_before = g.len();
            let cells_before = g.cell_count();
            let hits_before: BTreeSet<ItemId> = g.query(q).into_iter().collect();

            g.insert(id(n), r);

            prop_assert_eq!(g.len(), len_before, "len changed after re-inserting identical bounds for {:?}", id(n));
            prop_assert_eq!(
                g.cell_count(), cells_before,
                "cell_count changed after re-inserting identical bounds for {:?}", id(n)
            );
            let hits_after: BTreeSet<ItemId> = g.query(q).into_iter().collect();
            prop_assert_eq!(
                &hits_after, &hits_before,
                "query({:?}) changed after re-inserting identical bounds for {:?}: {:?} != {:?}",
                q, id(n), hits_after, hits_before,
            );
        }
    }

    // ── 7. GridHashIndex::new always clamps cell_size to at least 1.0 ──
    proptest! {
        #[test]
        fn new_clamps_cell_size_to_at_least_one(raw in -1.0e7f32..1.0e7f32) {
            let g = GridHashIndex::new(raw);
            prop_assert!(
                g.cell_size() >= 1.0,
                "cell_size({}) = {} is below the documented floor of 1.0",
                raw, g.cell_size(),
            );
            // And it should track the input exactly whenever the input
            // already satisfies the floor.
            if raw >= 1.0 {
                prop_assert_eq!(
                    g.cell_size(), raw,
                    "cell_size({}) was altered even though it already satisfied the floor", raw,
                );
            }
        }
    }

    // ── 8. query never panics, whatever rect it is asked about ──
    proptest! {
        #![proptest_config(ProptestConfig { cases: 1024, ..ProptestConfig::default() })]
        #[test]
        fn query_never_panics((cell_size, items, q) in arb_layout_and_query()) {
            let (g, _model) = build_index(cell_size, &items);
            // The property under test is "doesn't panic"; a successful
            // return (of any Vec, including empty) is the pass condition.
            let _ = g.query(q);
        }
    }

    // ── 9. widening the query rect can only add hits, never drop any ──
    proptest! {
        #![proptest_config(ProptestConfig { cases: 512, ..ProptestConfig::default() })]
        #[test]
        fn query_is_monotonic_in_rect_containment(
            (cell_size, items, outer) in arb_layout_and_query()
                .prop_map(|(cs, items, outer)| (cs, items, outer)),
            // Fractions (of outer's width/height) used to carve an inner
            // rect that's fully contained in `outer`.
            fx in 0.0f32..1.0,
            fy in 0.0f32..1.0,
            fw in 0.0f32..1.0,
            fh in 0.0f32..1.0,
        ) {
            // Guard against a degenerate outer rect (width/height 0 or
            // negative would make "inner ⊆ outer" ill-defined here).
            prop_assume!(outer.width > 0.0 && outer.height > 0.0);

            let (g, _model) = build_index(cell_size, &items);

            let inner_x = outer.x + fx * outer.width;
            let inner_y = outer.y + fy * outer.height;
            let inner_w = fw * (outer.right() - inner_x);
            let inner_h = fh * (outer.bottom() - inner_y);
            let inner = Rect::new(inner_x, inner_y, inner_w.max(0.0), inner_h.max(0.0));

            let inner_hits: BTreeSet<ItemId> = g.query(inner).into_iter().collect();
            let outer_hits: BTreeSet<ItemId> = g.query(outer).into_iter().collect();

            prop_assert!(
                inner_hits.is_subset(&outer_hits),
                "inner rect {:?} (inside outer {:?}) hit {:?} not returned by the wider query: {:?}",
                inner, outer,
                inner_hits.difference(&outer_hits).collect::<Vec<_>>(),
                outer_hits,
            );
        }
    }
}
