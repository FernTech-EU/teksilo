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

use bastyde_canvas::Rect;

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
    /// (on a tile) or a marquee (on the background), and to compute a 2D
    /// drag-reorder insertion index. Default scans via `tile_rect`.
    fn index_at_point(
        &self,
        content_point: bastyde_canvas::Point,
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::grid_view::layout::uniform::UniformGrid;
    use bastyde_canvas::{EdgeInsets, Point};

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
}
