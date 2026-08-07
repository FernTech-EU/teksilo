// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Sectioned uniform grid: a header row above each section's tile band.
//!
//! Wraps the uniform column geometry and interleaves a fixed-height header
//! before each section's tiles. The flat item index space is unchanged
//! (selection / keyboard nav stay flat); only the vertical layout gains the
//! per-section header offsets. Section item counts are read from a closure
//! each sync so the layout follows data changes.

use std::cell::RefCell;
use std::rc::Rc;

use teksilo_canvas::{EdgeInsets, Point};

use super::columns::{ColumnGeometry, column_at, geometry_for};
use super::strategy::{BUFFER_ROWS, GridLayoutStrategy, GridSizing, TileRect, VisibleTileRange};

/// Computed geometry for one section.
#[derive(Debug, Clone, Copy)]
struct SectionGeom {
    header_top: f32,
    band_top: f32,
    first_flat: usize,
    count: usize,
    rows: usize,
}

#[derive(Debug, Default)]
struct SectionLayout {
    cols: usize,
    counts: Vec<usize>,
    sections: Vec<SectionGeom>,
    total: f32,
    dirty: bool,
}

/// A uniform grid grouped into sections with fixed-height headers.
pub struct SectionedGrid {
    columns: ColumnGeometry,
    tile_height: f32,
    row_gap: f32,
    header_height: f32,
    inset: EdgeInsets,
    counts_fn: Rc<dyn Fn() -> Vec<usize>>,
    cache: RefCell<SectionLayout>,
}

impl std::fmt::Debug for SectionedGrid {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SectionedGrid")
            .field("sections", &self.cache.borrow().sections.len())
            .finish()
    }
}

impl SectionedGrid {
    pub(crate) fn new(
        sizing: GridSizing,
        col_gap: f32,
        row_gap: f32,
        inset: EdgeInsets,
        header_height: f32,
        counts_fn: Rc<dyn Fn() -> Vec<usize>>,
    ) -> Self {
        Self {
            columns: geometry_for(sizing, col_gap, inset),
            tile_height: sizing.tile_height().max(0.0),
            row_gap: row_gap.max(0.0),
            header_height: header_height.max(0.0),
            inset,
            counts_fn,
            cache: RefCell::new(SectionLayout::default()),
        }
    }

    fn row_step(&self) -> f32 {
        self.tile_height + self.row_gap
    }

    fn content_width(&self, viewport_width: f32) -> f32 {
        (viewport_width - self.inset.horizontal()).max(0.0)
    }

    /// Recompute the section layout if the column count or counts changed.
    fn sync(&self, viewport_width: f32) {
        let cols = self.columns.column_count(viewport_width).max(1);
        let counts = (self.counts_fn)();
        {
            let c = self.cache.borrow();
            if !c.dirty && c.cols == cols && c.counts == counts {
                return;
            }
        }
        let mut sections = Vec::with_capacity(counts.len());
        let mut y = self.inset.top;
        let mut first_flat = 0usize;
        for &count in &counts {
            let header_top = y;
            let band_top = y + self.header_height;
            let rows = count.div_ceil(cols);
            let band_h = if rows > 0 {
                rows as f32 * self.row_step() - self.row_gap
            } else {
                0.0
            };
            sections.push(SectionGeom {
                header_top,
                band_top,
                first_flat,
                count,
                rows,
            });
            // Advance past the band, leaving a section gap (reuse row_gap).
            y = band_top + band_h + self.row_gap;
            first_flat += count;
        }
        let total = if sections.is_empty() {
            0.0
        } else {
            (y - self.row_gap).max(self.inset.top) + self.inset.bottom
        };
        *self.cache.borrow_mut() = SectionLayout {
            cols,
            counts,
            sections,
            total,
            dirty: false,
        };
    }

    /// The section containing flat index `flat`. Skips empty sections (a
    /// `count == 0` section shares its `first_flat` with the next one and must
    /// not claim its neighbour's items).
    fn section_of(&self, flat: usize, cache: &SectionLayout) -> usize {
        let mut s = 0;
        for (i, g) in cache.sections.iter().enumerate() {
            if g.count > 0 && g.first_flat <= flat && flat < g.first_flat + g.count {
                return i;
            }
            if g.count > 0 && g.first_flat <= flat {
                s = i;
            }
        }
        s
    }
}

impl GridLayoutStrategy for SectionedGrid {
    fn column_count(&self, viewport_width: f32) -> usize {
        self.columns.column_count(viewport_width)
    }

    fn column_x(&self, col: usize, viewport_width: f32) -> (f32, f32) {
        self.columns.column_x(col, viewport_width)
    }

    fn total_content_height(&self, _item_count: usize, viewport_width: f32) -> f32 {
        self.sync(viewport_width);
        self.cache.borrow().total
    }

    fn visible_range(
        &self,
        scroll_y: f32,
        viewport_height: f32,
        viewport_width: f32,
        item_count: usize,
    ) -> VisibleTileRange {
        self.sync(viewport_width);
        if item_count == 0 {
            return VisibleTileRange { start: 0, end: 0 };
        }
        let cache = self.cache.borrow();
        let cols = cache.cols.max(1);
        let top = scroll_y;
        let bot = scroll_y + viewport_height;
        let mut min_flat: Option<usize> = None;
        let mut max_flat: Option<usize> = None;
        for g in &cache.sections {
            if g.count == 0 {
                continue;
            }
            let band_bottom = g.band_top + g.rows as f32 * self.row_step() - self.row_gap;
            if band_bottom < top || g.band_top > bot {
                continue;
            }
            // Rows of this band intersecting the viewport.
            let rel_top = (top - g.band_top).max(0.0);
            let first_row = (rel_top / self.row_step()).floor() as usize;
            let rel_bot = (bot - g.band_top).max(0.0);
            let last_row =
                ((rel_bot / self.row_step()).ceil() as usize).min(g.rows.saturating_sub(1));
            let lo = g.first_flat + first_row * cols;
            let hi = (g.first_flat + (last_row + 1) * cols).min(g.first_flat + g.count);
            min_flat = Some(min_flat.map_or(lo, |m| m.min(lo)));
            max_flat = Some(max_flat.map_or(hi, |m| m.max(hi)));
        }
        match (min_flat, max_flat) {
            (Some(lo), Some(hi)) => {
                let buf = BUFFER_ROWS * cols;
                VisibleTileRange {
                    start: lo.saturating_sub(buf),
                    end: (hi + buf).min(item_count),
                }
            }
            _ => VisibleTileRange { start: 0, end: 0 },
        }
    }

    fn tile_rect(&self, index: usize, viewport_width: f32) -> TileRect {
        self.sync(viewport_width);
        let cache = self.cache.borrow();
        let cols = cache.cols.max(1);
        let s = self.section_of(index, &cache);
        let g = cache.sections.get(s).copied().unwrap_or(SectionGeom {
            header_top: self.inset.top,
            band_top: self.inset.top,
            first_flat: 0,
            count: 0,
            rows: 0,
        });
        let local = index.saturating_sub(g.first_flat);
        let row = local / cols;
        let col = local % cols;
        let (x, width) = self.columns.column_x(col, viewport_width);
        let y = g.band_top + row as f32 * self.row_step();
        TileRect {
            x,
            y,
            width,
            height: self.tile_height,
        }
    }

    fn estimated_row_height(&self) -> f32 {
        self.tile_height
    }

    fn index_at_point(
        &self,
        content_point: Point,
        item_count: usize,
        viewport_width: f32,
    ) -> Option<usize> {
        if item_count == 0 {
            return None;
        }
        self.sync(viewport_width);
        let cache = self.cache.borrow();
        if cache.sections.is_empty() {
            return None;
        }
        let cols = cache.cols.max(1);
        // First section whose band is at/after the point; the containing
        // (or nearest-preceding) section is one back. `band_top` is
        // non-decreasing across sections, so this is a valid binary search.
        let after = cache
            .sections
            .partition_point(|g| g.band_top <= content_point.y);
        let g = cache.sections[after.saturating_sub(1)];
        if g.count == 0 {
            return None;
        }
        let rel_y = content_point.y - g.band_top;
        if rel_y < 0.0 {
            return None; // above this section's band (its header, or the gap before it)
        }
        let row = (rel_y / self.row_step()) as usize;
        if row >= g.rows || rel_y - row as f32 * self.row_step() > self.tile_height {
            return None; // past the section's last row, or a row-gap within it
        }
        let col = column_at(&self.columns, content_point.x, viewport_width)?;
        let local = row * cols + col;
        if local >= g.count {
            return None;
        }
        let idx = g.first_flat + local;
        (idx < item_count).then_some(idx)
    }

    fn tile_row_col(&self, index: usize, viewport_width: f32) -> (usize, usize) {
        self.sync(viewport_width);
        let cache = self.cache.borrow();
        let cols = cache.cols.max(1);
        let s = self.section_of(index, &cache);
        let g = cache.sections.get(s).copied().unwrap_or(SectionGeom {
            header_top: self.inset.top,
            band_top: self.inset.top,
            first_flat: 0,
            count: 0,
            rows: 0,
        });
        let local = index.saturating_sub(g.first_flat);
        (local / cols, local % cols)
    }

    fn headers_in_range(
        &self,
        scroll_y: f32,
        viewport_height: f32,
        viewport_width: f32,
    ) -> Vec<(usize, TileRect)> {
        self.sync(viewport_width);
        let cache = self.cache.borrow();
        let cw = self.content_width(viewport_width);
        let top = scroll_y - self.header_height;
        let bot = scroll_y + viewport_height;
        cache
            .sections
            .iter()
            .enumerate()
            .filter(|(_, g)| g.header_top >= top && g.header_top <= bot)
            .map(|(i, g)| {
                (
                    i,
                    TileRect {
                        x: self.inset.leading,
                        y: g.header_top,
                        width: cw,
                        height: self.header_height,
                    },
                )
            })
            .collect()
    }

    fn current_section(&self, scroll_y: f32, viewport_width: f32) -> Option<usize> {
        self.sync(viewport_width);
        let cache = self.cache.borrow();
        let mut current = None;
        for (i, g) in cache.sections.iter().enumerate() {
            if g.header_top <= scroll_y {
                current = Some(i);
            } else {
                break;
            }
        }
        current.or(if cache.sections.is_empty() {
            None
        } else {
            Some(0)
        })
    }

    fn header_rect(&self, section: usize, viewport_width: f32) -> Option<TileRect> {
        self.sync(viewport_width);
        let cache = self.cache.borrow();
        let cw = self.content_width(viewport_width);
        cache.sections.get(section).map(|g| TileRect {
            x: self.inset.leading,
            y: g.header_top,
            width: cw,
            height: self.header_height,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::rc::Rc;

    fn grid(counts: Vec<usize>) -> SectionedGrid {
        SectionedGrid::new(
            GridSizing::FixedColumnCount {
                count: 2,
                height: 50.0,
            },
            8.0,
            8.0,
            EdgeInsets::ZERO,
            20.0,
            Rc::new(move || counts.clone()),
        )
    }

    #[test]
    fn empty_leading_section_does_not_capture_first_item() {
        // counts [0, 2]: section 0 is empty. Section 0 header at y=0, its
        // (empty) band advances by header(20) + gap(8) = 28; section 1 header
        // at y=28, band at 48. Item 0 belongs to section 1, so its top is 48,
        // NOT section 0's band_top (20).
        let g = grid(vec![0, 2]);
        let r = g.tile_rect(0, 200.0);
        assert!((r.y - 48.0).abs() < 0.5, "item 0 y = {} (expected 48)", r.y);
    }

    #[test]
    fn index_at_point_finds_tile_in_second_section() {
        // counts [3, 3], 2 cols: section 1's band starts at y = 20(header) +
        // 108(section 0's 2-row band) + 8(gap) + 20(section 1's own header)
        // = 156.
        let g = grid(vec![3, 3]);
        assert_eq!(g.index_at_point(Point::new(0.0, 156.0), 6, 300.0), Some(3));
    }

    #[test]
    fn index_at_point_returns_none_between_sections() {
        let g = grid(vec![3, 3]);
        // y=140 sits between section 0's last row (ending at 128) and
        // section 1's tiles (starting at 156) — the row-gap plus section
        // 1's header — not a tile.
        assert_eq!(g.index_at_point(Point::new(0.0, 140.0), 6, 300.0), None);
    }

    #[test]
    fn tile_row_col_is_section_local() {
        // Item 3 is the FIRST item of section 1 (items 0, 1, 2 belong to
        // section 0), so it must report row 0 / col 0 within ITS OWN
        // section — not row 1 / col 1, the answer global `index / cols,
        // index % cols` math would give.
        let g = grid(vec![3, 3]);
        assert_eq!(g.tile_row_col(3, 300.0), (0, 0));
        assert_eq!(g.tile_row_col(4, 300.0), (0, 1));
        assert_eq!(g.tile_row_col(5, 300.0), (1, 0));
    }
}
