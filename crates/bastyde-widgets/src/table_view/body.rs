// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Per-row container widget — `Role::Row`, lays its cells horizontally
//! using a shared column-width handle owned by the parent table.
//!
//! The body widget itself is the `TableView` root; this file just holds
//! the small `BodyRow` container that one level above the leaf cell
//! delegates so the AccessKit tree exposes the canonical
//! `Table > Row > Cell` hierarchy.
//!
//! ## Pane bands and horizontal scroll
//!
//! When no column is pinned (`PaneBoundaries::leading_count == 0` and
//! `middle_end == cells.len()` — the overwhelmingly common case), `BodyRow`
//! keeps its original flat shape: cells are direct children, positioned by a
//! single cumulative walk offset by `-scroll_x`. Content scrolled out of
//! view is caught by the existing ancestor clips (`BodyPane` / `TableView`
//! both `clips_children()`), exactly as static column overflow always has
//! been — no new node, no behavior change for the default case.
//!
//! When pinning IS active, a scrolled-out-of-place Middle-pane cell could
//! otherwise paint over a co-resident Leading/Trailing-pane cell within the
//! same row bounds (the outer ancestor clip only bounds the row's own outer
//! edges, not the seam between panes). `build()` then groups the cells into
//! up to three `RowBand` children — Leading / Middle / Trailing — and only
//! the Middle band clips (`RowBand::clips_children`); Leading/Trailing never
//! need it since their own width IS the sum of their own columns. This is
//! the same "wrap in a `clips_children` container" idiom `ScrollArea` /
//! `BodyPane` / `TableView` already use, applied per-pane instead of
//! per-widget.

use std::cell::RefCell;
use std::rc::Rc;

use bastyde_canvas::{Point, Rect, Size, SizeProposal};
use bastyde_core::accessibility::AccessNodeBuilder;
use bastyde_core::build_context::BuildContext;
use bastyde_core::signal::Signal;
use bastyde_core::widget::{LayoutContext, Widget, WidgetPlacement};
use bastyde_core::widget_id::WidgetId;

use super::PaneBoundaries;
use super::layout::band_rects;

/// Shared handle holding the resolved column widths in display order.
/// `TableView` writes in `place_children` (after running the column
/// solver against the available width); each `BodyRow` reads in its own
/// `place_children` to position its cells. Sharing through `Rc<RefCell>`
/// keeps row layout consistent with the table's effective widths without
/// re-cloning the vector per row.
pub(crate) type SharedColumnWidths = Rc<RefCell<Vec<f32>>>;

/// Whether `PaneBoundaries` actually pins anything — the trigger for the
/// per-row band split. Shared between `BodyRow` and `HeaderRow` so the two
/// never disagree on when to switch shapes.
fn has_pinning(boundaries: PaneBoundaries, cell_count: usize) -> bool {
    boundaries.leading_count > 0 || boundaries.middle_end < cell_count
}

/// One row's worth of cells. Pre-built children are passed in by index;
/// `place_children` reads the shared widths and positions each cell at
/// `(sum(widths[0..i]), 0)` with `(widths[i], row_height)` — or, when
/// pinning splits the row into bands (see module docs), delegates to those
/// bands' own `place_children`.
#[derive(Debug)]
pub(crate) struct BodyRow {
    cells: Vec<WidgetId>,
    /// 1-based row index for AccessKit. Header is 1; first body row is 2.
    row_index_1based: usize,
    selected: bool,
    /// `Some(h)` — fixed height (uniform / exact modes). `None` —
    /// auto-measure: `layout_response` measures each cell at its column
    /// width and reports the tallest (height-for-width).
    row_height: Option<f32>,
    widths: SharedColumnWidths,
    /// Pane partition — see module docs. Snapshotted at construction: a
    /// pinning/order change bumps the owning table's rebuild version, so a
    /// fresh `BodyRow` (and fresh boundaries) is constructed on every change
    /// anyway.
    pane_boundaries: PaneBoundaries,
    /// The owning table's Middle-pane horizontal scroll offset. Forwarded
    /// to the Middle `RowBand` (via `RowBand::scrollable`) when pinning
    /// splits the row into bands; read directly in the flat, no-pinning
    /// path's own cumulative walk.
    scroll_x: Signal<f32>,
    /// When `false`, the row is invisible to AccessKit — used by
    /// TreeTableView, which wraps BodyRow in `TreeRowA11y` and wants the
    /// outer wrapper to carry `Role::Row` instead.
    announce_a11y: bool,

    // Build state — populated by `build()`.
    bands: Option<[Option<WidgetId>; 3]>,
}

impl BodyRow {
    pub(crate) fn new(
        cells: Vec<WidgetId>,
        row_index_1based: usize,
        selected: bool,
        row_height: Option<f32>,
        widths: SharedColumnWidths,
        pane_boundaries: PaneBoundaries,
        scroll_x: Signal<f32>,
    ) -> Self {
        Self {
            cells,
            row_index_1based,
            selected,
            row_height,
            widths,
            pane_boundaries,
            scroll_x,
            announce_a11y: true,
            bands: None,
        }
    }

    pub(crate) fn a11y_hidden(mut self) -> Self {
        self.announce_a11y = false;
        self
    }

    fn has_pinning(&self) -> bool {
        has_pinning(self.pane_boundaries, self.cells.len())
    }
}

impl Widget for BodyRow {
    fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
        if !self.has_pinning() {
            // Flat shape — cells are direct children (see module docs).
            return Vec::new();
        }
        let b = self.pane_boundaries;
        let leading_end = b.leading_count.min(self.cells.len());
        let middle_end = b.middle_end.min(self.cells.len()).max(leading_end);
        let leading: Vec<WidgetId> = self.cells[..leading_end].to_vec();
        let middle: Vec<WidgetId> = self.cells[leading_end..middle_end].to_vec();
        let trailing: Vec<WidgetId> = self.cells[middle_end..].to_vec();

        let mut bands: [Option<WidgetId>; 3] = [None, None, None];
        if !leading.is_empty() {
            bands[0] = Some(ctx.add(RowBand::new(leading, self.widths.clone(), 0)));
        }
        if !middle.is_empty() {
            bands[1] = Some(
                ctx.add(
                    RowBand::new(middle, self.widths.clone(), leading_end)
                        .scrollable(self.scroll_x.clone()),
                ),
            );
        }
        if !trailing.is_empty() {
            bands[2] = Some(ctx.add(RowBand::new(trailing, self.widths.clone(), middle_end)));
        }
        let out: Vec<WidgetId> = bands.iter().copied().flatten().collect();
        self.bands = Some(bands);
        out
    }

    fn layout_response(
        &self,
        proposal: SizeProposal,
        ctx: &LayoutContext,
    ) -> bastyde_core::widget::LayoutResponse {
        // Width: caller's proposal (the row fills its parent's bounds).
        let width = proposal
            .width
            .unwrap_or_else(|| self.widths.borrow().iter().sum());
        // Height: the configured row height, or — in auto-measure mode —
        // the tallest cell measured at its column width. The widths were
        // resolved by the table root's `place_children` earlier in this
        // same pass; the borrow is dropped before measuring. Measures
        // `self.cells` directly (not the bands) — cells are known
        // `WidgetId`s regardless of how `build()` grouped them, and
        // `ctx.child_size` works on any arena id.
        let height = match self.row_height {
            Some(h) => h,
            None => {
                let widths: Vec<f32> = self.widths.borrow().clone();
                let mut max_h = 0.0_f32;
                for (i, cell) in self.cells.iter().enumerate() {
                    let w = widths.get(i).copied().unwrap_or(width);
                    if let Some(size) = ctx.child_size(*cell, SizeProposal::with_width(w)) {
                        max_h = max_h.max(size.height);
                    }
                }
                max_h
            }
        };
        Size::new(width, height).into()
    }

    fn place_children(
        &self,
        bounds: Rect,
        _proposal: SizeProposal,
        children: &mut [WidgetPlacement],
        ctx: &LayoutContext,
    ) {
        if let Some(bands) = self.bands {
            let widths = self.widths.borrow();
            let rtl = ctx.is_rtl();
            let (leading_rect, middle_rect, trailing_rect) =
                band_rects(bounds, &widths, self.pane_boundaries, rtl);
            let rects = [leading_rect, middle_rect, trailing_rect];
            let mut next = 0;
            for (band, rect) in bands.iter().zip(rects.iter()) {
                if band.is_some() {
                    if let Some(child) = children.get_mut(next) {
                        child.origin = rect.origin();
                        child.size = rect.size();
                    }
                    next += 1;
                }
            }
            return;
        }

        let widths = self.widths.borrow();
        let total_children = children.len();
        // Defensive fallback: if the widths vector is shorter than the cell
        // count (shouldn't happen — TableView writes them in lock-step),
        // distribute evenly. Captured before `iter_mut` to avoid an
        // aliasing borrow.
        let fallback_w = if total_children == 0 {
            0.0
        } else {
            bounds.width / total_children as f32
        };
        let scroll = self.scroll_x.get();
        // Display order is preserved; only the physical x reverses under
        // RTL (the HStack model). Cell `i` is column display-index `i` in
        // both directions — the AT/selection/width contract is unchanged.
        if ctx.is_rtl() {
            let mut x = bounds.right() + scroll;
            for (i, child) in children.iter_mut().enumerate() {
                let w = widths.get(i).copied().unwrap_or(fallback_w);
                x -= w;
                child.origin = Point::new(x, bounds.y);
                child.size = Size::new(w, bounds.height);
            }
        } else {
            let mut x = bounds.x - scroll;
            for (i, child) in children.iter_mut().enumerate() {
                let w = widths.get(i).copied().unwrap_or(fallback_w);
                child.origin = Point::new(x, bounds.y);
                child.size = Size::new(w, bounds.height);
                x += w;
            }
        }
    }

    fn accessibility(&self, builder: &mut AccessNodeBuilder) {
        if !self.announce_a11y {
            builder.set_hidden();
            return;
        }
        builder.set_role(bastyde_core::accesskit::Role::Row);
        builder.set_selected(self.selected);
        builder.set_row_index(self.row_index_1based);
    }

    fn children(&self) -> Vec<WidgetId> {
        match self.bands {
            Some(bands) => bands.iter().copied().flatten().collect(),
            None => self.cells.clone(),
        }
    }
}

/// One pane band (Leading / Middle / Trailing) of cells within a header or
/// body row, used only when column pinning splits the row (see the module
/// docs on [`BodyRow`]). Groups a contiguous slice of already-built cell
/// widgets sharing the row-wide `SharedColumnWidths` handle, positions them
/// at `widths[widths_start..]` relative to its own bounds, and — only for
/// the Middle band, via [`scrollable`](Self::scrollable) — shifts that walk
/// by `-scroll_x` and clips its children so a partially-scrolled cell at
/// either edge is cropped to the band instead of bleeding into a pinned
/// neighbour.
#[derive(Debug)]
pub(crate) struct RowBand {
    cells: Vec<WidgetId>,
    widths: SharedColumnWidths,
    widths_start: usize,
    scroll_x: Option<Signal<f32>>,
}

impl RowBand {
    pub(crate) fn new(
        cells: Vec<WidgetId>,
        widths: SharedColumnWidths,
        widths_start: usize,
    ) -> Self {
        Self {
            cells,
            widths,
            widths_start,
            scroll_x: None,
        }
    }

    /// Mark this band as the scrollable Middle pane: it shifts its cells by
    /// `-scroll_x.get()` and clips them to its own bounds.
    pub(crate) fn scrollable(mut self, scroll_x: Signal<f32>) -> Self {
        self.scroll_x = Some(scroll_x);
        self
    }
}

impl Widget for RowBand {
    fn layout_response(
        &self,
        proposal: SizeProposal,
        _ctx: &LayoutContext,
    ) -> bastyde_core::widget::LayoutResponse {
        // Never queried for sizing purposes — `BodyRow`/`HeaderRow` compute
        // each band's rect themselves (`layout::band_rects`) and assign it
        // directly in their own `place_children`, the same way `RowBand`
        // assigns its own children's rects below. Row height in
        // auto-measure mode is measured off the raw cell ids directly
        // (`BodyRow::layout_response`), bypassing this widget entirely.
        proposal.resolve(0.0, 0.0).into()
    }

    fn place_children(
        &self,
        bounds: Rect,
        _proposal: SizeProposal,
        children: &mut [WidgetPlacement],
        ctx: &LayoutContext,
    ) {
        let widths = self.widths.borrow();
        let scroll = self.scroll_x.as_ref().map(|s| s.get()).unwrap_or(0.0);
        if ctx.is_rtl() {
            let mut x = bounds.right() + scroll;
            for (i, child) in children.iter_mut().enumerate() {
                let w = widths.get(self.widths_start + i).copied().unwrap_or(0.0);
                x -= w;
                child.origin = Point::new(x, bounds.y);
                child.size = Size::new(w, bounds.height);
            }
        } else {
            let mut x = bounds.x - scroll;
            for (i, child) in children.iter_mut().enumerate() {
                let w = widths.get(self.widths_start + i).copied().unwrap_or(0.0);
                child.origin = Point::new(x, bounds.y);
                child.size = Size::new(w, bounds.height);
                x += w;
            }
        }
    }

    fn children(&self) -> Vec<WidgetId> {
        self.cells.clone()
    }

    fn clips_children(&self) -> bool {
        self.scroll_x.is_some()
    }
}
