//! Per-row container widget — `Role::Row`, lays its cells horizontally
//! using a shared column-width handle owned by the parent table.
//!
//! The body widget itself is the `TableView` root; this file just holds
//! the small `BodyRow` container that one level above the leaf cell
//! delegates so the AccessKit tree exposes the canonical
//! `Table > Row > Cell` hierarchy.

use std::cell::RefCell;
use std::rc::Rc;

use fern_canvas::{Point, Rect, Size, SizeProposal};
use fern_core::accessibility::AccessNodeBuilder;
use fern_core::widget::{LayoutContext, Widget, WidgetPlacement};
use fern_core::widget_id::WidgetId;

/// Shared handle holding the resolved column widths in display order.
/// `TableView` writes in `place_children` (after running the column
/// solver against the available width); each `BodyRow` reads in its own
/// `place_children` to position its cells. Sharing through `Rc<RefCell>`
/// keeps row layout consistent with the table's effective widths without
/// re-cloning the vector per row.
pub(crate) type SharedColumnWidths = Rc<RefCell<Vec<f32>>>;

/// One row's worth of cells. Pre-built children are passed in by index;
/// `place_children` reads the shared widths and positions each cell at
/// `(sum(widths[0..i]), 0)` with `(widths[i], row_height)`.
#[derive(Debug)]
pub(crate) struct BodyRow {
    cells: Vec<WidgetId>,
    /// 1-based row index for AccessKit. Header is 1; first body row is 2.
    row_index_1based: usize,
    selected: bool,
    row_height: f32,
    widths: SharedColumnWidths,
    /// When `false`, the row is invisible to AccessKit — used by
    /// TreeTable, which wraps BodyRow in `TreeRowA11y` and wants the
    /// outer wrapper to carry `Role::Row` instead.
    announce_a11y: bool,
}

impl BodyRow {
    pub(crate) fn new(
        cells: Vec<WidgetId>,
        row_index_1based: usize,
        selected: bool,
        row_height: f32,
        widths: SharedColumnWidths,
    ) -> Self {
        Self {
            cells,
            row_index_1based,
            selected,
            row_height,
            widths,
            announce_a11y: true,
        }
    }

    pub(crate) fn a11y_hidden(mut self) -> Self {
        self.announce_a11y = false;
        self
    }
}

impl Widget for BodyRow {
    fn layout_response(
        &self,
        proposal: SizeProposal,
        _ctx: &LayoutContext,
    ) -> fern_core::widget::LayoutResponse {
        // Width: caller's proposal (the row fills its parent's bounds).
        // Height: the configured row height.
        let width = proposal
            .width
            .unwrap_or_else(|| self.widths.borrow().iter().sum());
        Size::new(width, self.row_height).into()
    }

    fn place_children(
        &self,
        bounds: Rect,
        _proposal: SizeProposal,
        children: &mut [WidgetPlacement],
        _ctx: &LayoutContext,
    ) {
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
        let mut x = bounds.x;
        for (i, child) in children.iter_mut().enumerate() {
            let w = widths.get(i).copied().unwrap_or(fallback_w);
            child.origin = Point::new(x, bounds.y);
            child.size = Size::new(w, bounds.height);
            x += w;
        }
    }

    fn accessibility(&self, builder: &mut AccessNodeBuilder) {
        if !self.announce_a11y {
            builder.set_hidden();
            return;
        }
        builder.set_role(fern_core::accesskit::Role::Row);
        builder.set_selected(self.selected);
        builder.inner_mut().set_row_index(self.row_index_1based);
    }

    fn children(&self) -> Vec<WidgetId> {
        self.cells.clone()
    }
}
