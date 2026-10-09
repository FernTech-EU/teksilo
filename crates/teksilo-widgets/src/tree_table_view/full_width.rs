// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Full-width rows for `TreeTableView`: a row drawn as one cell across every
//! column — a group row such as "an artist · 12 songs" above rows that do
//! fill the columns.
//!
//! The row keeps everything that makes it a row of the tree — its place in
//! the flattening, its selection, its chevron, its drag — and swaps only what
//! is drawn: one delegate-built band instead of one cell per column. So the
//! body pane builds it through the same path as any row and differs at the
//! cells; the keyboard learns of it through
//! [`RowNavigator::spans_all_columns`](crate::table_view::row_navigator::RowNavigator::spans_all_columns).
//!
//! **The band stays in the viewport.** It is laid across the body's width and
//! ignores the horizontal scroll, so a group row stays readable while the
//! columns beside it scroll. Its chevron sits where the tree column's would
//! with the columns unscrolled: under the twist of the rows around it while
//! the tree column is the first one (the default) or pinned, and a column
//! scroll away from it otherwise.

use std::rc::Rc;

use teksilo_canvas::{Point, Rect, Size, SizeProposal};
use teksilo_core::accessibility::AccessNodeBuilder;
use teksilo_core::widget::{LayoutContext, Widget, WidgetPlacement};
use teksilo_core::widget_id::WidgetId;

use crate::table_view::PaneBoundaries;
use crate::table_view::body::SharedColumnWidths;
use crate::table_view::column::CellContext;
use crate::table_view::layout;

/// Builds a full-width row's band from its item.
pub(crate) type BandDelegate<T> = Rc<dyn Fn(&T, &CellContext) -> Box<dyn Widget>>;

/// Which rows are drawn full width, and what draws them — resolved once per
/// build of the view and shared by the body pane and the pinned copies.
pub(crate) struct FullWidthRows<T: 'static> {
    /// Whether the visible row at a flat index is a full-width row. `false`
    /// for a row that is not resident yet: a loading row has no item to ask.
    pub(crate) at: Rc<dyn Fn(usize) -> bool>,
    /// The band's content: the application's delegate, or the tree column's
    /// own cell when it set none.
    pub(crate) delegate: BandDelegate<T>,
}

impl<T: 'static> Clone for FullWidthRows<T> {
    fn clone(&self) -> Self {
        Self {
            at: self.at.clone(),
            delegate: self.delegate.clone(),
        }
    }
}

/// A full-width row: one cell, laid across the whole row.
///
/// Publishes no node of its own, like a `TreeTableView` `BodyRow`: the
/// `TreeRowA11y` around it carries the row.
#[derive(Debug)]
pub(crate) struct FullWidthRow {
    cell: WidgetId,
    /// `Some(h)` in the uniform and callback modes; `None` measures the band
    /// at the row's width (auto-measure mode).
    row_height: Option<f32>,
}

impl FullWidthRow {
    pub(crate) fn new(cell: WidgetId, row_height: Option<f32>) -> Self {
        Self { cell, row_height }
    }
}

impl Widget for FullWidthRow {
    fn layout_response(
        &self,
        proposal: SizeProposal,
        ctx: &LayoutContext,
    ) -> teksilo_core::widget::LayoutResponse {
        let measured = || {
            let at_width = SizeProposal {
                width: proposal.width,
                height: None,
            };
            ctx.child_size(self.cell, at_width)
        };
        let width = proposal
            .width
            .unwrap_or_else(|| measured().map_or(0.0, |s| s.width));
        let height = match self.row_height {
            Some(h) => h,
            None => measured().map_or(0.0, |s| s.height),
        };
        Size::new(width, height).into()
    }

    fn place_children(
        &self,
        bounds: Rect,
        _proposal: SizeProposal,
        children: &mut [WidgetPlacement],
        _ctx: &LayoutContext,
    ) {
        for child in children.iter_mut() {
            child.origin = bounds.origin();
            child.size = bounds.size();
        }
    }

    fn accessibility(&self, builder: &mut AccessNodeBuilder) {
        // Dropped by every adapter, the cell promoted to the row above.
        builder.set_role(teksilo_core::accesskit::Role::GenericContainer);
    }

    fn children(&self) -> Vec<WidgetId> {
        vec![self.cell]
    }
}

/// Puts a band's content (indent, chevron, delegate) at the tree column's
/// leading edge, measured as if the columns were not scrolled.
///
/// Read at layout time from the shared column widths, the same handle the
/// rows read: the widths are resolved by the view's own placement, which runs
/// before any row is laid out.
#[derive(Debug)]
pub(crate) struct BandInset {
    child: WidgetId,
    widths: SharedColumnWidths,
    boundaries: PaneBoundaries,
    tree_display_pos: usize,
}

impl BandInset {
    pub(crate) fn new(
        child: WidgetId,
        widths: SharedColumnWidths,
        boundaries: PaneBoundaries,
        tree_display_pos: usize,
    ) -> Self {
        Self {
            child,
            widths,
            boundaries,
            tree_display_pos,
        }
    }

    /// The tree column's offset from the row's leading edge, for a row
    /// `width` wide.
    fn lead(&self, width: f32) -> f32 {
        let widths = self.widths.borrow();
        layout::column_logical_x(&widths, self.boundaries, 0.0, width, self.tree_display_pos)
            .unwrap_or(0.0)
            .clamp(0.0, width.max(0.0))
    }
}

impl Widget for BandInset {
    fn layout_response(
        &self,
        proposal: SizeProposal,
        ctx: &LayoutContext,
    ) -> teksilo_core::widget::LayoutResponse {
        match proposal.width {
            Some(width) => {
                let lead = self.lead(width);
                let inner = SizeProposal {
                    width: Some((width - lead).max(0.0)),
                    height: proposal.height,
                };
                let height = ctx.child_size(self.child, inner).map_or(0.0, |s| s.height);
                Size::new(width, height).into()
            }
            None => {
                let natural = ctx.child_size(self.child, proposal).unwrap_or(Size::ZERO);
                Size::new(natural.width, natural.height).into()
            }
        }
    }

    fn place_children(
        &self,
        bounds: Rect,
        _proposal: SizeProposal,
        children: &mut [WidgetPlacement],
        ctx: &LayoutContext,
    ) {
        let lead = self.lead(bounds.width);
        let width = (bounds.width - lead).max(0.0);
        // The leading edge is the right one under RTL, where the columns run
        // from the right.
        let x = if ctx.is_rtl() {
            bounds.x
        } else {
            bounds.x + lead
        };
        for child in children.iter_mut() {
            child.origin = Point::new(x, bounds.y);
            child.size = Size::new(width, bounds.height);
        }
    }

    fn children(&self) -> Vec<WidgetId> {
        vec![self.child]
    }
}
