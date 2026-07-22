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
//! - **Oversized items.** An item whose AABB would bucket into more
//!   than `MAX_CELLS_PER_ITEM` grid cells (a scene backdrop, a
//!   full-document canvas rect, or any item at extreme coordinates
//!   with large bounds — all reachable in production, not exotic) is
//!   NOT bucketed cell-by-cell at all. It is stored instead in a
//!   separate `oversized: HashMap<ItemId, Rect>` that `query`
//!   always scans in full, in addition to the cell lookup, keeping
//!   an exact AABB-intersection test against `scene_rect` (so it
//!   contributes no cell-fan-out false positives of its own).
//!
//!   This closes what used to be an unconditional, uncapped eager
//!   allocation: `cells_for_rect` computed
//!   `(width / cell_size) * (height / cell_size)` cells and reserved
//!   that many `(i32, i32)` slots *before* the loop that fills them
//!   ran — no upper bound, and using bare `i32` arithmetic that could
//!   itself overflow for large extents (debug builds panicked,
//!   release builds could wrap to a huge or negative `usize`). A
//!   single 1e6 × 1e6 logical-pixel item at the clamped-minimum
//!   `cell_size` of 1.0 asked for `(1e6+1)² ≈ 1e12` cells — roughly
//!   8 TB for the `Vec<(i32, i32)>` alone — before any assertion or
//!   even the fill loop ran; this was reachable from a single
//!   `Scene::add_item` call, no adversarial input required. Even at
//!   the default 256 px `cell_size`, a 1e6-square item alone reserved
//!   `(1e6 / 256)² ≈ 1.5e7` cells (~122 MB) for that one item. The
//!   same hazard applied to `query`/`items_in_rect`, since a caller
//!   can pass an arbitrarily large `scene_rect` too — see `query`'s
//!   own oversized-span fallback.
//!
//!   A custom `SpatialIndex` would still handle non-uniform density
//!   better — an R-tree, say, for an editor with many overlapping
//!   items — but none ships; the trait is the place to add one.
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

/// Cap on how many grid cells a single item's AABB may be bucketed
/// into before it is instead stored in the always-scanned `oversized`
/// list (see [`GridHashIndex::insert`] and the module doc's
/// "Oversized items" section).
///
/// Chosen as a small constant that keeps the bucketed fast path's
/// worst-case per-item footprint bounded and independent of the
/// item's actual size: at `MAX_CELLS_PER_ITEM` cells, the worst case
/// is `MAX_CELLS_PER_ITEM` entries in `item_cells` (a
/// `Vec<(i32, i32)>`, 8 bytes per entry) plus up to
/// `MAX_CELLS_PER_ITEM` distinct single-item buckets in `cells` (a
/// `HashMap` entry + a `Vec<ItemId>` each, tens of bytes) — on the
/// order of 40–50 KB for one pathologically-shaped item, versus the
/// previous unconditional and unbounded reservation described above.
///
/// 1024 is generous headroom above typical scene content: at the
/// default `cell_size` of 256 px that's an ~8192×8192 px square item
/// before it goes oversized; at the clamped minimum `cell_size` of
/// 1.0 px (see [`GridHashIndex::new`]) that's a mere ~32×32 px item.
/// Anything bigger at that cell size is exactly the shape of the bug
/// this constant fixes: the incident's 1e6 × 1e6 item at
/// `cell_size: 1.0` (`(1e6+1)² ≈ 1e12` cells, ~8 TB, under the old
/// code) now falls straight into `oversized` instead.
const MAX_CELLS_PER_ITEM: u64 = 1024;

/// AABB intersection test used by [`GridHashIndex::query`]'s
/// oversized-item scan. `mod tests` / `mod proptests` below keep their
/// own independent copies for brute-force cross-checks — deliberately
/// not sharing this one, so a bug here couldn't be masked by a test
/// using the same implementation to verify itself.
fn rects_intersect(a: Rect, b: Rect) -> bool {
    a.x < b.x + b.width && b.x < a.x + a.width && a.y < b.y + b.height && b.y < a.y + a.height
}

/// Uniform grid spatial hash. Each item is bucketed into every cell
/// its AABB overlaps; queries union all items from the cells the
/// query rect overlaps. Items whose AABB would span more than
/// `MAX_CELLS_PER_ITEM` cells are NOT bucketed — see `oversized`
/// below and the module doc's "Oversized items" section.
#[derive(Debug)]
pub struct GridHashIndex {
    cell_size: f32,
    cells: HashMap<(i32, i32), Vec<ItemId>>,
    /// Reverse lookup so `remove` and `insert` (as update) don't need
    /// to scan every cell.
    item_cells: HashMap<ItemId, Vec<(i32, i32)>>,
    /// Items whose AABB spans more than `MAX_CELLS_PER_ITEM` grid
    /// cells. Never bucketed into `cells`/`item_cells` — `query`
    /// always scans this map in full instead, checking a true AABB
    /// intersection against the query rect. An `ItemId` is present in
    /// exactly one of `item_cells` or `oversized` at any time, never
    /// both (`insert` always calls `remove` first).
    oversized: HashMap<ItemId, Rect>,
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
            oversized: HashMap::new(),
        }
    }

    /// The configured cell size in logical pixels.
    pub fn cell_size(&self) -> f32 {
        self.cell_size
    }

    /// Number of cells currently storing at least one item. Useful
    /// for diagnostics; not part of the public `SpatialIndex` trait.
    /// Oversized items (see `MAX_CELLS_PER_ITEM`) never occupy a
    /// cell, so they never contribute to this count.
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

    /// Test-only accessor: `true` if `id` is currently stored in the
    /// `oversized` representation rather than bucketed into `cells`.
    /// Mirrors `has_empty_bucket` — not part of the public API, added
    /// so the proptest suite can assert on which representation an
    /// item landed in without making `oversized` `pub`.
    #[cfg(test)]
    fn is_oversized(&self, id: ItemId) -> bool {
        self.oversized.contains_key(&id)
    }

    /// The inclusive grid-cell span `[min_x, max_x] × [min_y, max_y]`
    /// that `r` covers, using the half-open convention: a rect that
    /// ends exactly on a cell boundary doesn't include the next cell.
    /// Otherwise an item sitting on a boundary would over-bucket and
    /// queries would double-count.
    fn cell_span_for_rect(&self, r: Rect) -> (i32, i32, i32, i32) {
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
        (min_x, min_y, max_x, max_y)
    }

    /// Number of grid cells the inclusive span `[min_x, max_x] ×
    /// [min_y, max_y]` covers.
    ///
    /// Computed via `i64` subtraction promoted to a saturating `u64`
    /// multiplication — never the bare `i32 * i32` product the
    /// original bug used, which could itself overflow for
    /// large-extent rects (debug: panic; release: wrap, possibly to a
    /// negative value that then reinterpreted as a huge `usize`). This
    /// function is pure arithmetic — O(1) and allocation-free — so it
    /// is always safe to call, even with a span that would be
    /// catastrophic to actually enumerate.
    fn cell_span_count(min_x: i32, min_y: i32, max_x: i32, max_y: i32) -> u64 {
        let width = (i64::from(max_x) - i64::from(min_x) + 1).max(1) as u64;
        let height = (i64::from(max_y) - i64::from(min_y) + 1).max(1) as u64;
        width.saturating_mul(height)
    }

    /// Whether `r`'s grid-cell span exceeds `MAX_CELLS_PER_ITEM` —
    /// i.e. whether it must go into `oversized` instead of being
    /// bucketed cell-by-cell. See the module doc's "Oversized items"
    /// section.
    fn rect_is_oversized(&self, r: Rect) -> bool {
        let (min_x, min_y, max_x, max_y) = self.cell_span_for_rect(r);
        Self::cell_span_count(min_x, min_y, max_x, max_y) > MAX_CELLS_PER_ITEM
    }

    /// Enumerate every grid cell `r` covers.
    ///
    /// Precondition upheld by both call sites — `insert` (only after
    /// `rect_is_oversized` returns `false`) and `query`'s normal-path
    /// branch (only after its own span-count check) — is that `r`'s
    /// span is `<= MAX_CELLS_PER_ITEM` cells, a small constant. The
    /// `debug_assert!` below exists to catch a future call site that
    /// forgets that precondition during development/test, rather than
    /// silently reintroducing the original unbounded-allocation bug in
    /// release builds.
    fn cells_for_rect(&self, r: Rect) -> Vec<(i32, i32)> {
        let (min_x, min_y, max_x, max_y) = self.cell_span_for_rect(r);
        let count = Self::cell_span_count(min_x, min_y, max_x, max_y);
        debug_assert!(
            count <= MAX_CELLS_PER_ITEM,
            "cells_for_rect called with a span of {count} cells, above MAX_CELLS_PER_ITEM \
             ({MAX_CELLS_PER_ITEM}) for rect {r:?} — callers must route anything this large \
             through the `oversized` representation instead of enumerating cells for it",
        );
        let mut out = Vec::with_capacity(count as usize);
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
        // Re-insert: drop any previous bucketed OR oversized entry
        // first, so an id can move freely between the two
        // representations (normal→oversized and oversized→normal) on
        // a bounds change.
        self.remove(id);
        if self.rect_is_oversized(bounds) {
            self.oversized.insert(id, bounds);
            return;
        }
        let cells = self.cells_for_rect(bounds);
        for cell in &cells {
            self.cells.entry(*cell).or_default().push(id);
        }
        self.item_cells.insert(id, cells);
    }

    fn remove(&mut self, id: ItemId) {
        if self.oversized.remove(&id).is_some() {
            return;
        }
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

        let (min_x, min_y, max_x, max_y) = self.cell_span_for_rect(scene_rect);
        let span_count = Self::cell_span_count(min_x, min_y, max_x, max_y);

        if span_count <= MAX_CELLS_PER_ITEM {
            // Normal path: the query rect itself covers a bounded
            // number of cells (same cap as a single item), so
            // enumerating them directly is cheap.
            for cell in self.cells_for_rect(scene_rect) {
                if let Some(items) = self.cells.get(&cell) {
                    for &id in items {
                        if seen.insert(id) {
                            result.push(id);
                        }
                    }
                }
            }
        } else {
            // The QUERY rect itself spans more cells than any single
            // item is allowed to occupy — e.g. a "select everything"
            // or fit-to-content query over a huge area. Enumerating
            // min_x..=max_x × min_y..=max_y directly here would hit
            // the exact unbounded-allocation hazard `MAX_CELLS_PER_ITEM`
            // exists to close for items, just on the query side
            // instead. So instead scan the (much smaller) set of
            // POPULATED cells and keep only the ones inside the span
            // — O(populated cells) rather than O(cells the rect
            // covers). Populated-cell count is bounded by
            // `items_in_the_grid × MAX_CELLS_PER_ITEM`, never by the
            // query rect's area, so this is always safe. The result is
            // identical to the normal path's (same set of cells
            // considered — just discovered from the other direction).
            for (&(cx, cy), items) in &self.cells {
                if (min_x..=max_x).contains(&cx) && (min_y..=max_y).contains(&cy) {
                    for &id in items {
                        if seen.insert(id) {
                            result.push(id);
                        }
                    }
                }
            }
        }

        // Oversized items are never bucketed into `cells` at all, so
        // they must always be checked directly — regardless of which
        // branch above ran — against a true AABB intersection. This
        // is what keeps the never-under-report invariant for an item
        // too big to cell-bucket, and (since it's an exact check, not
        // a cell-fan-out approximation) it never contributes a false
        // positive of its own.
        for (&id, &bounds) in &self.oversized {
            if rects_intersect(bounds, scene_rect) && seen.insert(id) {
                result.push(id);
            }
        }

        // Stable order so query results are deterministic across
        // runs — useful for tests and reproducible debugging.
        result.sort_unstable();
        result
    }

    fn contains(&self, id: ItemId) -> bool {
        self.item_cells.contains_key(&id) || self.oversized.contains_key(&id)
    }

    fn len(&self) -> usize {
        self.item_cells.len() + self.oversized.len()
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

    #[test]
    fn oversized_1e6_extent_at_cell_size_one_never_allocates_the_pathological_cell_count() {
        // The literal incident input: a 1e6 x 1e6 logical-pixel rect at
        // cell_size 1.0 used to make `cells_for_rect` reserve
        // `(1e6+1) * (1e6+1) ~= 1e12` cells — about 8 TB for a
        // `Vec<(i32, i32)>` alone (and risked an i32*i32 overflow along
        // the way). It must now be classified oversized and never touch
        // `cells`/`item_cells` at all.
        let mut g = GridHashIndex::new(1.0);
        let item = id(1);
        g.insert(item, Rect::new(0.0, 0.0, 1_000_000.0, 1_000_000.0));
        assert!(
            g.is_oversized(item),
            "the reboot-inducing input must be classified oversized"
        );
        assert!(g.contains(item));
        assert_eq!(g.len(), 1);
        assert_eq!(
            g.cell_count(),
            0,
            "an oversized item must not touch the cell buckets"
        );

        // It must still be findable by a query that truly intersects it.
        let hits = g.query(Rect::new(500.0, 500.0, 10.0, 10.0));
        assert_eq!(hits, vec![item]);
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
    /// Cap on how many cells a single generated item may span per axis, for
    /// the GENERATORS that stress the ordinary bucketed fast path (item
    /// count vs. query correctness, insertion-order independence, etc.).
    ///
    /// Historical note — this constant is the reason the resource-
    /// exhaustion bug was caught at all: `GridHashIndex::insert` used to
    /// allocate one `(i32, i32)` per covered cell (see `cells_for_rect`)
    /// with NO upper bound, so an unconstrained extent combined with the
    /// smallest `cell_size` this suite generates (1.0) asked for
    /// `1e6 * 1e6 = 1e12` cells and OOMed the machine before any assertion
    /// ran — which is how this suite came to take a developer's
    /// workstation down. That was a REAL BUG in `cells_for_rect`, not
    /// merely a bad generator: a scene that adds one very large item (a
    /// backdrop, a full-document canvas rect) with a small `cell_size`
    /// would hang or OOM in production, no adversarial input required.
    ///
    /// `GridHashIndex` now fixes this directly: an item (or a query rect)
    /// whose span exceeds `MAX_CELLS_PER_ITEM` (1024) is never bucketed
    /// cell-by-cell — see the module doc's "Oversized items" section and
    /// the dedicated properties below (10–13) that exercise that path
    /// specifically, including the exact 1e6-at-`cell_size:1.0` incident
    /// input. So it would now be SAFE to relax or remove this cap — no
    /// combination of `cell_size` and extent can OOM or overflow the index
    /// anymore, per the arithmetic on `MAX_CELLS_PER_ITEM`.
    ///
    /// It is kept at 64 anyway, deliberately, for a coverage reason
    /// unrelated to safety: `arb_extent`'s "a few cells" branch draws
    /// `(1.0..MAX_CELLS_PER_AXIS)`, and properties 1, 2, 4, 5, 6, 8, 9 lean
    /// on that branch to stress the NORMAL bucketed path's boundary/
    /// precision arithmetic (the 2–64-cells-per-axis regime is where an
    /// off-by-one or an f32 rounding slip in `cells_for_rect` would show
    /// up). Raising this constant to, say, `1_000_000.0` to match
    /// `arb_coord`'s large-magnitude branch would make that "a few cells"
    /// branch draw an almost-always-oversized value instead (a uniform
    /// draw over `[1, 1e6)` puts less than 0.1% of its mass below 1024),
    /// silently starving those seven properties of the multi-cell-bucketing
    /// coverage they exist for. Rather than dilute that shared generator,
    /// the oversized path gets its own dedicated generators in properties
    /// 10–13 below, which is why this constant is unchanged.
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

    // Side length (in logical pixels) of a square rect guaranteed to exceed
    // MAX_CELLS_PER_ITEM at the given cell_size, for ANY cell_size this
    // suite's arb_cell_size() produces (1.0..=256.0): a
    // `sqrt(MAX_CELLS_PER_ITEM)`-cells-per-axis square already sits right at
    // the cap, so doubling it clears the cap comfortably regardless of
    // f32-rounding at the boundary.
    fn oversized_side_for(cell_size: f32) -> f32 {
        cell_size * (MAX_CELLS_PER_ITEM as f32).sqrt() * 2.0
    }

    // ── 10. an oversized item is still found by a query that truly intersects it ──
    proptest! {
        #![proptest_config(ProptestConfig { cases: 256, ..ProptestConfig::default() })]
        #[test]
        fn oversized_item_is_found_by_a_query_that_truly_intersects_it(
            cell_size in arb_cell_size(),
            n in 0u64..8,
            // Fractions used to carve a query rect that is a strict
            // sub-rect of (hence guaranteed to truly intersect) the
            // oversized item's bounds.
            fx in 0.0f32..1.0,
            fy in 0.0f32..1.0,
            fw in 0.01f32..1.0,
            fh in 0.01f32..1.0,
        ) {
            let side = oversized_side_for(cell_size);
            let big_rect = Rect::new(0.0, 0.0, side, side);

            let mut g = GridHashIndex::new(cell_size);
            let item = id(n);
            g.insert(item, big_rect);
            prop_assert!(
                g.is_oversized(item),
                "cell_size={} side={}: item should have been classified oversized (cap={})",
                cell_size, side, MAX_CELLS_PER_ITEM,
            );

            // A rect confined to [0, side/2) x [0, side/2) with modest
            // width/height is always a strict sub-rect of big_rect, hence
            // always truly intersects it.
            let qx = fx * side * 0.5;
            let qy = fy * side * 0.5;
            let qw = (fw * side * 0.25).max(0.01);
            let qh = (fh * side * 0.25).max(0.01);
            let query_rect = Rect::new(qx, qy, qw, qh);

            let hits = g.query(query_rect);
            prop_assert!(
                hits.contains(&item),
                "cell_size={} query={:?}: oversized item {:?} (bounds {:?}) missed by a query \
                 that truly intersects it — a lost click through the oversized path",
                cell_size, query_rect, item, big_rect,
            );
        }
    }

    // ── 11. re-insert moves an item between normal and oversized with no ghost in either representation ──
    proptest! {
        #![proptest_config(ProptestConfig { cases: 256, ..ProptestConfig::default() })]
        #[test]
        fn reinsert_moves_between_normal_and_oversized_with_no_ghost(
            cell_size in arb_cell_size(),
            n in 0u64..8,
        ) {
            let small_side = cell_size.min(10.0);
            let small_rect = Rect::new(0.0, 0.0, small_side, small_side);
            let side = oversized_side_for(cell_size);
            // Far enough from the origin that big_rect can never overlap
            // small_rect, so any hit at the "wrong" location is
            // unambiguously a ghost, not a coincidental true intersection.
            let far = cell_size * 10_000.0;
            let big_rect = Rect::new(far, far, side, side);

            let mut g = GridHashIndex::new(cell_size);
            let item = id(n);

            // 1. Normal path.
            g.insert(item, small_rect);
            prop_assert!(!g.is_oversized(item), "expected the small rect to bucket normally");
            prop_assert!(g.query(small_rect).contains(&item));

            // 2. normal -> oversized.
            g.insert(item, big_rect);
            prop_assert!(
                g.is_oversized(item),
                "cell_size={} side={}: expected the big rect to be classified oversized",
                cell_size, side,
            );
            prop_assert!(
                !g.query(small_rect).contains(&item),
                "ghost: {:?} still found at the old (normally-bucketed) rect {:?} after moving \
                 to the oversized rect {:?}",
                item, small_rect, big_rect,
            );
            prop_assert!(g.query(big_rect).contains(&item));

            // 3. oversized -> normal.
            g.insert(item, small_rect);
            prop_assert!(
                !g.is_oversized(item),
                "expected re-insert with a small rect to leave the oversized representation"
            );
            prop_assert!(
                !g.query(big_rect).contains(&item),
                "ghost: {:?} still found at the old oversized rect {:?} after moving back to {:?}",
                item, big_rect, small_rect,
            );
            prop_assert!(g.query(small_rect).contains(&item));
            prop_assert_eq!(g.len(), 1, "exactly one logical item should exist throughout");
        }
    }

    // ── 12. contains/len still track a HashMap model when oversized items are mixed in ──
    fn arb_rect_possibly_oversized(cell_size: f32) -> impl Strategy<Value = Rect> {
        prop_oneof![
            3 => arb_rect(cell_size),
            1 => {
                let side = oversized_side_for(cell_size);
                (-10i32..10i32, -10i32..10i32).prop_map(move |(kx, ky)| {
                    Rect::new(kx as f32 * side, ky as f32 * side, side, side)
                })
            },
        ]
    }

    fn arb_op_possibly_oversized(cell_size: f32) -> impl Strategy<Value = Op> {
        prop_oneof![
            3 => (0u64..12, arb_rect_possibly_oversized(cell_size)).prop_map(|(n, r)| Op::Insert(n, r)),
            1 => (0u64..12).prop_map(Op::Remove),
        ]
    }

    proptest! {
        #![proptest_config(ProptestConfig { cases: 512, ..ProptestConfig::default() })]
        #[test]
        fn contains_and_len_track_a_hashmap_model_with_oversized_items_mixed_in(
            (cell_size, ops) in arb_cell_size()
                .prop_flat_map(|cs| (Just(cs), prop::collection::vec(arb_op_possibly_oversized(cs), 0..80)))
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
                    "after {:?} (oversized items mixed in): index len {} != model len {} \
                     (ops so far: {:?})",
                    op, g.len(), model.len(), ops,
                );
            }

            // Full-state check over the bounded id range every op draws
            // from, since there's no "list all ids" accessor.
            for n in 0..12u64 {
                prop_assert_eq!(
                    g.contains(id(n)), model.contains_key(&id(n)),
                    "id {} contains()={} but model has_key={} after ops {:?} \
                     (oversized items mixed in)",
                    n, g.contains(id(n)), model.contains_key(&id(n)), ops,
                );
            }
        }
    }

    // ── 13. a single item's cell footprint never exceeds MAX_CELLS_PER_ITEM, for any extent ──
    //
    // This is the property that would have prevented the reboots: it
    // exercises the exact incident input (a 1e6 extent at cell_size 1.0,
    // via the `Just(1_000_000.0_f32)` branch below) alongside other large
    // extents and the suite's usual cell sizes, and checks the observable
    // proxy for "did this allocate a pathological number of cells" —
    // `cell_count()` staying within the cap regardless of how the item was
    // classified.
    proptest! {
        #![proptest_config(ProptestConfig { cases: 512, ..ProptestConfig::default() })]
        #[test]
        fn a_single_items_cell_footprint_never_exceeds_the_per_item_cap(
            cell_size in arb_cell_size(),
            extent in prop_oneof![
                Just(1_000_000.0_f32),
                Just(500_000.0_f32),
                Just(100_000.0_f32),
                (1.0f32..MAX_CELLS_PER_ITEM as f32 * 4.0),
            ],
        ) {
            let mut g = GridHashIndex::new(cell_size);
            let item = id(1);
            g.insert(item, Rect::new(0.0, 0.0, extent, extent));

            prop_assert!(
                g.cell_count() <= MAX_CELLS_PER_ITEM as usize,
                "cell_size={} extent={}: cell_count()={} exceeds the per-item cap {} — the \
                 resource-exhaustion bug is back",
                cell_size, extent, g.cell_count(), MAX_CELLS_PER_ITEM,
            );
            prop_assert!(g.contains(item));
            prop_assert_eq!(g.len(), 1);
        }
    }
}
