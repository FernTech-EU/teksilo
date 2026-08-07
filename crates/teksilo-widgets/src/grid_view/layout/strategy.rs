// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! The pluggable layout strategy that drives `GridView<T>`'s virtualization.
//!
//! A `GridLayoutStrategy` answers every geometric question the
//! virtualization engine needs — how many columns fit a viewport, the
//! content-space rect of any item, the flat index range to realize for a
//! scroll offset, and the total content height. Three concrete strategies
//! ship:
//!
//! * [`UniformGrid`](super::uniform::UniformGrid) — fixed tile size /
//!   fixed column count / adaptive min-width. Exact O(1) positions.
//! * `VariableRowGrid` — each row sized to its tallest tile (auto-measure
//!   + scroll-anchoring, or an exact `item_height(index)` fast-path).
//! * `VirtualizedMasonry` — Pinterest-style column-balanced waterfall.
//!
//! Keeping the engine behind this trait means the body pane, scrollbar
//! wiring, keyboard nav, and accessibility never need to know which layout
//! is active.

use std::ops::Range;

use teksilo_canvas::Rect;

/// The default over-realization window: this many extra rows are built
/// above and below the viewport so a small scroll doesn't trigger a
/// rebuild (only a relayout). Mirrors `ListView`/`TableView`.
pub(crate) const BUFFER_ROWS: usize = 5;

/// Where a programmatically-revealed item should land in the viewport.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ScrollAnchor {
    /// Minimum scroll that makes the item fully visible (no-op if already
    /// visible). The default for keyboard navigation.
    #[default]
    Auto,
    /// The item's leading edge aligns with the viewport top.
    Start,
    /// The item is centered in the viewport.
    Center,
    /// The item's trailing edge aligns with the viewport bottom.
    End,
}

/// Tile sizing policy. Names mirror SwiftUI `GridItem`, Flutter's
/// `SliverGridDelegate`, and WinUI `MinItemWidth`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum GridSizing {
    /// Every tile is exactly `width` × `height`. The column count is
    /// derived: as many `width`-wide tiles as fit the viewport. Tiles are
    /// NOT stretched — leftover space trails after the last column.
    Fixed { width: f32, height: f32 },
    /// Exactly `count` columns, each stretched to an equal share of the
    /// viewport width. Tile height is fixed at `height`.
    FixedColumnCount { count: usize, height: f32 },
    /// Fit as many columns as possible such that each tile is at least
    /// `min_width` wide; tiles stretch to fill, clamped to `max_width`
    /// when set (Flutter `maxCrossAxisExtent`). Tile height is `height`.
    Adaptive {
        min_width: f32,
        max_width: Option<f32>,
        height: f32,
    },
}

impl GridSizing {
    /// The fixed tile height carried by every variant.
    pub(crate) fn tile_height(&self) -> f32 {
        match *self {
            GridSizing::Fixed { height, .. }
            | GridSizing::FixedColumnCount { height, .. }
            | GridSizing::Adaptive { height, .. } => height,
        }
    }
}

/// The content-space rect of one tile (before the scroll offset is
/// subtracted). `x`/`y` are relative to the scrollable content origin.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TileRect {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}

/// The flat model-index range `[start, end)` to realize for a given scroll
/// + viewport, including the buffer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VisibleTileRange {
    pub start: usize,
    pub end: usize,
}

/// The geometry + virtualization contract every grid layout implements.
///
/// Object-safe (`Rc<dyn GridLayoutStrategy>`). Strategies use interior
/// mutability for their height caches so the `&self` `place_children`
/// pass can feed measured heights back (see [`observe_measured`]).
///
/// [`observe_measured`]: GridLayoutStrategy::observe_measured
pub(crate) trait GridLayoutStrategy: std::fmt::Debug + 'static {
    /// Number of columns for `viewport_width`. Must be O(1) — called every
    /// frame in `place_children`.
    fn column_count(&self, viewport_width: f32) -> usize;

    /// The `(x, width)` of column `col` within `viewport_width`. `col` must
    /// be `< column_count(viewport_width)`.
    fn column_x(&self, col: usize, viewport_width: f32) -> (f32, f32);

    /// Total scrollable content height for `item_count` items. May be an
    /// estimate for variable-height strategies before every row has been
    /// measured. Drives `max_scroll_y` and the scrollbar thumb ratio.
    fn total_content_height(&self, item_count: usize, viewport_width: f32) -> f32;

    /// Total content width. The default (fill the viewport, no horizontal
    /// overflow) is correct for every vertical-scroll strategy.
    fn total_content_width(&self, viewport_width: f32) -> f32 {
        viewport_width
    }

    /// Flat index range `[start, end)` to realize, including the buffer.
    fn visible_range(
        &self,
        scroll_y: f32,
        viewport_height: f32,
        viewport_width: f32,
        item_count: usize,
    ) -> VisibleTileRange;

    /// Content-space rect of item `index`. `index` must be `< item_count`.
    fn tile_rect(&self, index: usize, viewport_width: f32) -> TileRect;

    /// The height used for rows that have not been measured yet (also the
    /// fixed height for uniform strategies). Used to size the buffer.
    fn estimated_row_height(&self) -> f32;

    // ── Variable-height hooks (no-ops for `UniformGrid`) ────────────────

    /// Whether the body pane should measure each realized tile's
    /// height-for-width and feed it back via [`observe_measured`]. Uniform
    /// strategies return `false` (heights are fixed).
    ///
    /// [`observe_measured`]: GridLayoutStrategy::observe_measured
    fn measures_tiles(&self) -> bool {
        false
    }

    /// Feed back the measured `(flat_index, height)` of every realized tile
    /// for one layout pass. The strategy folds them into its height cache
    /// (row-max for variable rows, per-column for waterfall) and returns
    /// the **scroll-anchor delta**: how far `scroll_y` must move to keep the
    /// content at/above the viewport top visually stationary. Returns `0.0`
    /// when nothing changed.
    fn observe_measured(
        &self,
        _measured: &[(usize, f32)],
        _scroll_y: f32,
        _viewport_width: f32,
    ) -> f32 {
        0.0
    }

    /// Invalidate cached heights for the flat item range (back to the
    /// estimate). Called on data changes; for inserts/removes pass
    /// `start..usize::MAX` because the grid reflows from the edit point.
    fn invalidate_rows(&self, _item_range: Range<usize>) {}

    /// Resize the internal height cache to `item_count` items (after an
    /// insert/remove/reset). No-op for uniform strategies.
    fn resize(&self, _item_count: usize) {}

    // ── Scroll-into-view + marquee ──────────────────────────────────────

    /// The signed scroll delta needed to satisfy `anchor` for `index`.
    /// `Auto` returns `0.0` when the item is already fully visible. The
    /// default implementation works for every strategy via `tile_rect`.
    fn scroll_delta_to_reveal(
        &self,
        index: usize,
        scroll_y: f32,
        viewport_height: f32,
        viewport_width: f32,
        anchor: ScrollAnchor,
    ) -> f32 {
        let r = self.tile_rect(index, viewport_width);
        let tile_top = r.y;
        let tile_bot = r.y + r.height;
        match anchor {
            ScrollAnchor::Start => tile_top - scroll_y,
            ScrollAnchor::End => tile_bot - viewport_height - scroll_y,
            ScrollAnchor::Center => (tile_top + r.height * 0.5) - viewport_height * 0.5 - scroll_y,
            ScrollAnchor::Auto => {
                if tile_top < scroll_y {
                    tile_top - scroll_y
                } else if tile_bot > scroll_y + viewport_height {
                    tile_bot - (scroll_y + viewport_height)
                } else {
                    0.0
                }
            }
        }
    }

    /// Flat indices whose tile rect intersects `content_rect` (a rubber-band
    /// rectangle in content space). Geometric — tests items outside the
    /// realized window too. The default scans every item via `tile_rect`,
    /// which is correct but O(n); strategies with a cheap row/column index
    /// may override for large datasets.
    fn hit_indices_in_rect(
        &self,
        content_rect: Rect,
        item_count: usize,
        viewport_width: f32,
    ) -> Vec<usize> {
        let mut hits = Vec::new();
        for i in 0..item_count {
            let r = self.tile_rect(i, viewport_width);
            let tile = Rect::new(r.x, r.y, r.width, r.height);
            if rects_intersect(content_rect, tile) {
                hits.push(i);
            }
        }
        hits
    }

    /// The flat index of the tile whose rect contains `content_point` (a
    /// point in content space), or `None` for an inter-tile gap / empty
    /// background. Used to decide whether a press should start an item drag
    /// (on a tile) or a marquee (on the background). Default scans via
    /// `tile_rect` — O(n); `UniformGrid`/`VariableRowGrid`/`SectionedGrid`
    /// override with a closed-form lookup since this runs on every
    /// `on_drag_hover` move. `VirtualizedMasonry` keeps this default: its
    /// placement isn't row-major (items drop into the currently-shortest
    /// column), so there's no O(1) inverse — acceptable for the
    /// hundreds-to-low-thousands of items a waterfall gallery holds.
    fn index_at_point(
        &self,
        content_point: teksilo_canvas::Point,
        item_count: usize,
        viewport_width: f32,
    ) -> Option<usize> {
        for i in 0..item_count {
            let r = self.tile_rect(i, viewport_width);
            if Rect::new(r.x, r.y, r.width, r.height).contains(content_point) {
                return Some(i);
            }
        }
        None
    }

    /// The flat index a drag-reorder drop at `content_point` should insert
    /// *before* — the counterpart to [`index_at_point`](Self::index_at_point)
    /// for drop resolution. Unlike `index_at_point` (which must return `None`
    /// for a background point so marquee-vs-drag disambiguation works),
    /// this ALWAYS resolves to a real insertion point: a point over a tile
    /// lands on its leading or trailing edge (by which half of the tile's
    /// width it falls in); a point in a gap (row-gap, column-gap, or before
    /// the first row) resolves to the nearest tile by row proximity first,
    /// then column proximity, and applies the same edge rule to it — so a
    /// row-gap point never silently falls through to "append at end" the
    /// way naively delegating to `index_at_point` would. Only a point at or
    /// past the bottom of the very last tile yields `item_count` (append).
    fn insertion_index_at(
        &self,
        content_point: teksilo_canvas::Point,
        item_count: usize,
        viewport_width: f32,
    ) -> usize {
        if item_count == 0 {
            return 0;
        }
        let last = self.tile_rect(item_count - 1, viewport_width);
        if content_point.y >= last.y + last.height {
            return item_count;
        }
        if let Some(i) = self.index_at_point(content_point, item_count, viewport_width) {
            let r = self.tile_rect(i, viewport_width);
            return if content_point.x > r.x + r.width * 0.5 {
                (i + 1).min(item_count)
            } else {
                i
            };
        }
        // Gap: the nearest tile by (vertical, then horizontal) edge
        // distance — 0 when the point is already within the tile's span on
        // that axis. Locking onto the nearest ROW first (not just the
        // nearest tile overall) is what makes a row-gap point resolve to
        // the adjacent row instead of an arbitrary far-away tile.
        let mut best = 0usize;
        let mut best_dy = f32::MAX;
        let mut best_dx = f32::MAX;
        for i in 0..item_count {
            let r = self.tile_rect(i, viewport_width);
            let dy = edge_gap(content_point.y, r.y, r.height);
            let dx = edge_gap(content_point.x, r.x, r.width);
            if dy < best_dy - 0.01 || ((dy - best_dy).abs() <= 0.01 && dx < best_dx) {
                best = i;
                best_dy = dy;
                best_dx = dx;
            }
        }
        let r = self.tile_rect(best, viewport_width);
        if content_point.x > r.x + r.width * 0.5 {
            (best + 1).min(item_count)
        } else {
            best
        }
    }

    /// `(row, col)` of item `index`, 0-based — the tile's ARIA grid
    /// coordinates and the values handed to the delegate via
    /// [`TileContext`](super::super::TileContext). Default is global
    /// row-major math (`index / cols`, `index % cols`), correct for every
    /// strategy whose flat index order matches its visual row order
    /// (uniform, variable-row, waterfall-as-appropriate). `SectionedGrid`
    /// overrides with SECTION-LOCAL numbering, since each section starts a
    /// fresh row band (see `SectionedGrid::tile_rect`) — the global index
    /// misreports row/col whenever an earlier section's count isn't a
    /// column multiple.
    fn tile_row_col(&self, index: usize, viewport_width: f32) -> (usize, usize) {
        let cols = self.column_count(viewport_width).max(1);
        (index / cols, index % cols)
    }

    // ── Section headers (only the sectioned strategy implements these) ──

    /// `(section_index, header_rect)` for every section header in the visible
    /// vertical range. Empty for non-sectioned strategies. The body pane
    /// realizes these header widgets alongside tiles.
    fn headers_in_range(
        &self,
        _scroll_y: f32,
        _viewport_height: f32,
        _viewport_width: f32,
    ) -> Vec<(usize, TileRect)> {
        Vec::new()
    }

    /// The section whose band the viewport top currently sits in (drives the
    /// sticky pinned header). `None` for non-sectioned strategies.
    fn current_section(&self, _scroll_y: f32, _viewport_width: f32) -> Option<usize> {
        None
    }

    /// The header rect for a single section (used to size the pinned slot).
    fn header_rect(&self, _section: usize, _viewport_width: f32) -> Option<TileRect> {
        None
    }
}

/// Axis-aligned rectangle intersection test (touching edges don't count).
pub(crate) fn rects_intersect(a: Rect, b: Rect) -> bool {
    a.x < b.right() && b.x < a.right() && a.y < b.bottom() && b.y < a.bottom()
}

/// Distance from `p` to the nearest edge of the span `[origin, origin +
/// extent]`; `0.0` when `p` falls inside it. The building block for
/// [`GridLayoutStrategy::insertion_index_at`]'s gap-resolution scan.
fn edge_gap(p: f32, origin: f32, extent: f32) -> f32 {
    if p < origin {
        origin - p
    } else if p > origin + extent {
        p - (origin + extent)
    } else {
        0.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::grid_view::layout::uniform::UniformGrid;
    use teksilo_canvas::{EdgeInsets, Point};

    fn grid() -> UniformGrid {
        // 100×50 tiles, 0 gaps, no insets → 4 columns in 400px.
        UniformGrid::new(
            GridSizing::Fixed {
                width: 100.0,
                height: 50.0,
            },
            0.0,
            0.0,
            EdgeInsets::ZERO,
        )
    }

    fn gapped_grid() -> UniformGrid {
        // 100×50 tiles, 10px gaps, no insets → 4 columns in 430px
        // (4*100 + 3*10 = 430), row_step = 50 + 10 = 60.
        UniformGrid::new(
            GridSizing::Fixed {
                width: 100.0,
                height: 50.0,
            },
            10.0,
            10.0,
            EdgeInsets::ZERO,
        )
    }

    #[test]
    fn hit_indices_in_rect_selects_intersecting_tiles() {
        let g = grid();
        // Rect over the first two columns of the first two rows: tiles
        // (0,1) in row 0 and (4,5) in row 1.
        let rect = Rect::new(10.0, 10.0, 150.0, 60.0);
        let mut hits = g.hit_indices_in_rect(rect, 40, 400.0);
        hits.sort();
        assert_eq!(hits, vec![0, 1, 4, 5]);
    }

    #[test]
    fn index_at_point_finds_tile_and_gap() {
        let g = grid();
        // Point inside tile 2 (x 200..300, y 0..50).
        assert_eq!(
            g.index_at_point(Point::new(250.0, 25.0), 40, 400.0),
            Some(2)
        );
        // Point in row 1, column 0 → index 4.
        assert_eq!(g.index_at_point(Point::new(10.0, 60.0), 40, 400.0), Some(4));
        // Point beyond the last item.
        assert_eq!(g.index_at_point(Point::new(10.0, 9000.0), 40, 400.0), None);
    }

    #[test]
    fn insertion_index_at_row_gap_does_not_fall_through_to_len() {
        // 12 items, 4 cols → 3 rows. y=53 sits in the row-gap between row 0
        // (0..50) and row 1 (60..110), closer to row 0; x=50 is inside
        // column 0. Before the fix this always fell through to `len` (12)
        // because `index_at_point` returns None for any non-tile point.
        let g = gapped_grid();
        let idx = g.insertion_index_at(Point::new(50.0, 53.0), 12, 430.0);
        assert!(
            idx < 12,
            "a mid-grid row-gap point must not fall through to len, got {idx}"
        );
    }

    #[test]
    fn insertion_index_at_col_gap_yields_next_tile() {
        // Row 0: tile 0 spans x 0..100, the gap spans 100..110, tile 1
        // spans 110..210. A point in the gap (x=105) must insert BEFORE
        // tile 1 — i.e. resolve to index 1 — not fall through to `len`.
        let g = gapped_grid();
        let idx = g.insertion_index_at(Point::new(105.0, 25.0), 12, 430.0);
        assert_eq!(
            idx, 1,
            "a point in the col-gap between tiles 0 and 1 should insert before tile 1"
        );
    }

    #[test]
    fn insertion_index_at_past_last_tile_yields_len() {
        let g = gapped_grid();
        let idx = g.insertion_index_at(Point::new(50.0, 9000.0), 12, 430.0);
        assert_eq!(
            idx, 12,
            "a point past the last tile should append at the end"
        );
    }
}
