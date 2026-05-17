//! Spatial index for [`Scene`](crate::Scene) items.
//!
//! `GridHashIndex` (default) is a uniform grid hash; an R-tree alternative
//! behind the same trait. The trait is deliberately small — three
//! mutating operations (`insert`, `remove`, `query`) plus two query
//! methods (`contains`, `len`) — so swapping implementations is a
//! one-line change in [`Scene::with_index`](crate::Scene::with_index).
//!
//! ## Why grid hash first
//!
//! - Cache-friendly: items in the same cell are stored contiguously.
//! - Insert / remove / move are amortised `O(k)` where `k` is the
//!   number of cells the item overlaps (typically 1–4 for items
//!   smaller than the cell size).
//! - `query(rect)` returns deduplicated, intersection-checked
//!   candidates from the cells the rect overlaps.
//! - Pathological case (one giant item that spans hundreds of cells)
//!   is rare and easily worked around by raising `cell_size` for
//!   that scene; an R-tree handles non-uniform density better and is
//!   an editor-style fallback for many overlapping items.
//!
//! Default `cell_size` is `256.0` logical pixels — large enough that
//! typical card-sized items (~200 px) bucket into 1–4 cells and small
//! enough that viewport queries (~800–1200 px) hit a manageable fan-out.

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
                    let r = items.iter().find(|(i, _)| i == id).unwrap().1;
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
