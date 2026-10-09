// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Pinned ancestor rows ("sticky scroll") for `TreeTableView`.
//!
//! While the view scrolls through a subtree, the rows that subtree hangs from
//! stay pinned under the header, outermost first, down to the depth the
//! application asked for: the artist, then the album, over the songs. The
//! mechanics are `ListView`'s pinned section header (`list_view/sections.rs`)
//! taken from one header to a stack of them:
//!
//! - a line is pushed up by the first row after its subtree as that row
//!   arrives under it, so a line never covers a row it is not an ancestor of;
//! - the copies are rebuilt for the rows the view names, a frame after the
//!   scroll that named them, and a line is shown only while its copy was
//!   built for the row named for it — never the previous row's content over
//!   this one's;
//! - the copies are pictures: hidden from assistive technology (the real
//!   rows stay where they are in the tree), out of the Tab order, and a press
//!   lands on the line as a whole, never on a control drawn inside it.
//!
//! ## Which rows, and where
//!
//! [`stack_at`] decides it from the flattening and the row geometry alone.
//! Line `k` is looked for under the lines above it: the row at that height is
//! found, and its ancestor at depth `k` is the row pinned there — the row
//! itself when it is an open branch whose top has scrolled under the line. A
//! row shallower than `k`, or a depth-`k` row sitting exactly in its slot,
//! ends the stack. A line is looked for under the *unpushed* lines above it,
//! so once a subtree has ended above that height the row found there is
//! shallower, and the stack ends: every line is a descendant of the one above.
//!
//! [`reveal_inset`] answers the other way round: how much of the top a row's
//! own ancestors cover when it is the first row below them. Every reveal the
//! view makes — the keyboard, `ensure_row_visible`, `scroll_to_row`, focus,
//! the ScrollIntoView action, a click on a pinned line — stops that far below
//! the top, so a copy never covers the row the cursor is on.

use std::cell::RefCell;
use std::rc::Rc;

use teksilo_canvas::{Canvas, Point, Rect, Size, SizeProposal};
use teksilo_core::accessibility::AccessNodeBuilder;
use teksilo_core::binding::BindingLevel;
use teksilo_core::build_context::BuildContext;
use teksilo_core::signal::Signal;
use teksilo_core::widget::{EventContext, LayoutContext, PaintContext, Widget, WidgetPlacement};
use teksilo_core::widget_builder::HandlerSet;
use teksilo_core::widget_id::WidgetId;
use teksilo_tokens::SurfaceRole;

use super::body_pane::tree_chrome;
use super::full_width::{BandInset, FullWidthRow, FullWidthRows};
use crate::common::row_metrics::{RowMetrics, SharedRowMetrics};
use crate::table_view::PaneBoundaries;
use crate::table_view::body::{BodyRow, SharedColumnWidths};
use crate::table_view::column::{CellContext, Column};
use crate::tree_source::TreeSource;

/// A row's top is "in its slot" within this distance, so a row scrolled into
/// place by a reveal is not pinned over itself for a rounding error.
const IN_PLACE_EPSILON: f32 = 0.5;

/// What the stack needs to know about the flattened tree, by visible flat
/// index.
pub(crate) trait Outline {
    fn row_count(&self) -> usize;
    /// `None` for a row that is not resident yet.
    fn depth(&self, row: usize) -> Option<usize>;
    /// Whether the row has children and shows them.
    fn is_open_branch(&self, row: usize) -> bool;
    /// The row's parent, which a pre-order flattening always shows above it.
    fn parent(&self, row: usize) -> Option<usize>;
}

impl<T: 'static> Outline for TreeSource<T> {
    fn row_count(&self) -> usize {
        self.visible_count()
    }

    fn depth(&self, row: usize) -> Option<usize> {
        self.meta(row).map(|m| m.depth)
    }

    fn is_open_branch(&self, row: usize) -> bool {
        self.meta(row)
            .is_some_and(|m| m.has_children && m.is_expanded)
    }

    fn parent(&self, row: usize) -> Option<usize> {
        // `meta`, not `depth`: `TreeSource::depth` reads a missing row as a root.
        let depth = self.meta(row)?.depth;
        if depth == 0 {
            return None;
        }
        // The source answers by key in O(1) on every built-in backing. A
        // minimal external source may not know a key's flat index; the nearest
        // shallower row above is the parent there too, by construction.
        self.parent_index(row).or_else(|| {
            (0..row)
                .rev()
                .find(|&i| self.meta(i).is_some_and(|m| m.depth < depth))
        })
    }
}

/// One line of the stack: the row it shows, its top (relative to the top of
/// the rows' viewport) and its height, which is the real row's.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct StackLine {
    pub(crate) row: usize,
    /// In `(natural - height, natural]`, `natural` being the bottom of the
    /// lines above: below it only by being pushed up.
    pub(crate) top: f32,
    pub(crate) height: f32,
}

/// The ancestor at depth `k` of `row`, which sits at `depth`.
fn ancestor_at(outline: &dyn Outline, row: usize, depth: usize, k: usize) -> Option<usize> {
    let (mut at, mut d) = (row, depth);
    while d > k {
        let parent = outline.parent(at)?;
        let parent_depth = outline.depth(parent)?;
        // A source whose parents do not get shallower would never end this.
        if parent_depth >= d {
            return None;
        }
        (at, d) = (parent, parent_depth);
    }
    (d == k).then_some(at)
}

/// The pinned stack at content offset `scroll`, at most `limit` lines deep.
/// See the module docs.
pub(crate) fn stack_at(
    outline: &dyn Outline,
    metrics: &mut RowMetrics,
    scroll: f32,
    limit: usize,
) -> Vec<StackLine> {
    let count = outline.row_count();
    let mut lines = Vec::new();
    if count == 0 || limit == 0 {
        return lines;
    }
    let total = metrics.total_height(count);
    let scroll = scroll.max(0.0);
    // The bottom of the lines found so far, unpushed.
    let mut slot = 0.0_f32;
    for k in 0..limit {
        let y = scroll + slot;
        if y >= total {
            break;
        }
        let probe = metrics.row_at(y);
        let Some(depth) = outline.depth(probe) else {
            break;
        };
        if depth < k {
            break;
        }
        let row = if depth == k {
            // In its slot, the row is the line already; a leaf or a closed
            // branch hangs nothing below it to pin it over.
            if !outline.is_open_branch(probe) || metrics.row_top(probe) >= y - IN_PLACE_EPSILON {
                break;
            }
            probe
        } else {
            match ancestor_at(outline, probe, depth, k) {
                Some(ancestor) => ancestor,
                None => break,
            }
        };
        let height = metrics.row_height(row);
        // The first row after `row`'s subtree pushes the line up as it
        // arrives under it. Only the rows the line's own extent reaches can
        // be that row, so the walk stops at the line's bottom: a few rows, not
        // the rest of the subtree.
        let mut top = slot;
        let mut next = probe + 1;
        while next < count {
            let next_top = metrics.row_top(next);
            if next_top >= y + height {
                break;
            }
            // A row not resident yet has no depth to compare; reading it as a
            // root's ends every subtree, which can only push too early.
            if outline.depth(next).is_none_or(|d| d <= k) {
                top = top.min(next_top - scroll - height);
                break;
            }
            next += 1;
        }
        lines.push(StackLine { row, top, height });
        slot += height;
    }
    lines
}

/// How much of the top of the viewport `row`'s ancestors cover when `row` is
/// the first row below them: the heights of its ancestors at the pinned
/// depths, `0` for a root.
pub(crate) fn reveal_inset(
    outline: &dyn Outline,
    metrics: &mut RowMetrics,
    row: usize,
    limit: usize,
) -> f32 {
    let Some(depth) = outline.depth(row) else {
        return 0.0;
    };
    let pinned_depths = limit.min(depth);
    let mut inset = 0.0;
    let (mut at, mut d) = (row, depth);
    while d > 0 {
        let Some(parent) = outline.parent(at) else {
            break;
        };
        let Some(parent_depth) = outline.depth(parent) else {
            break;
        };
        if parent_depth >= d {
            break;
        }
        if parent_depth < pinned_depths {
            inset += metrics.row_height(parent);
        }
        (at, d) = (parent, parent_depth);
    }
    inset
}

/// The offset that reveals `row` with `inset` of the top covered above it:
/// unchanged when it is already clear, its top right under the covered band
/// when it is above or under it, its bottom at the viewport bottom when it is
/// below — but never so far that its top goes under the band, when the row
/// and its ancestors are taller than the viewport.
pub(crate) fn scroll_for_reveal(
    metrics: &mut RowMetrics,
    row: usize,
    inset: f32,
    scroll: f32,
    viewport: f32,
    max_scroll: f32,
) -> f32 {
    let top = metrics.row_top(row);
    let bottom = top + metrics.row_height(row);
    let clear = top - inset;
    if clear < scroll {
        clear.max(0.0)
    } else if bottom > scroll + viewport {
        (bottom - viewport)
            .min(clear)
            .clamp(0.0, max_scroll.max(0.0))
    } else {
        scroll
    }
}

/// Everything a reveal below the pinned stack needs, cloneable into the
/// handlers that make one. Exists only while the view pins ancestors.
#[derive(Clone)]
pub(crate) struct PinnedReveal {
    outline: Rc<dyn Outline>,
    metrics: SharedRowMetrics,
    limit: usize,
}

impl PinnedReveal {
    pub(crate) fn new(
        outline: Rc<dyn Outline>,
        metrics: SharedRowMetrics,
        limit: usize,
    ) -> Option<Self> {
        (limit > 0).then_some(Self {
            outline,
            metrics,
            limit,
        })
    }

    /// The offset that reveals `row` clear of its ancestors' copies.
    ///
    /// `try_borrow_mut`, as `table_view::imperative` does: the metrics are
    /// also borrowed during layout, so a call from inside a delegate could
    /// re-enter. Leaving the offset alone beats panicking.
    pub(crate) fn scroll_for(&self, row: usize, scroll: f32, viewport: f32, max: f32) -> f32 {
        let count = self.outline.row_count();
        let Ok(mut m) = self.metrics.try_borrow_mut() else {
            return scroll;
        };
        m.resize(count);
        let inset = reveal_inset(&*self.outline, &mut m, row, self.limit);
        scroll_for_reveal(&mut m, row, inset, scroll, viewport, max)
    }

    /// The offset that puts `row` at the top of the viewport, under its
    /// ancestors' copies.
    pub(crate) fn top_for(&self, row: usize) -> Option<f32> {
        let count = self.outline.row_count();
        let mut m = self.metrics.try_borrow_mut().ok()?;
        m.resize(count);
        let inset = reveal_inset(&*self.outline, &mut m, row, self.limit);
        Some(m.row_top(row) - inset)
    }
}

/// Builds the copy of a row: the row's cells as the body pane lays them, or
/// its band when it is a full-width row, with no node published, no editor,
/// no drag and an inert chevron.
pub(crate) struct RowCopies<T: 'static> {
    pub(crate) source: Rc<TreeSource<T>>,
    pub(crate) columns: Vec<Column<T>>,
    pub(crate) display_indices: Vec<usize>,
    pub(crate) column_widths: SharedColumnWidths,
    pub(crate) pane_boundaries: PaneBoundaries,
    pub(crate) scroll_x: Signal<f32>,
    pub(crate) tree_display_pos: usize,
    pub(crate) indent_per_level: f32,
    pub(crate) row_metrics: SharedRowMetrics,
    pub(crate) full_width: Option<FullWidthRows<T>>,
}

impl<T: 'static> RowCopies<T> {
    /// The copy of `row`, or `None` when it is not resident.
    ///
    /// A copy shows the row's content, not its state: its delegates are told
    /// the row is neither selected nor focused, as no selection band or focus
    /// ring is drawn behind it either. Its height is the real row's, as the
    /// metrics hold it, so a copy lines up with the row it stands for.
    fn build(&self, ctx: &mut BuildContext, row: usize) -> Option<WidgetId> {
        let meta = self.source.meta(row)?;
        let height = self.row_metrics.borrow_mut().row_height(row);
        let tree_col = self.display_indices.get(self.tree_display_pos).copied();
        let context = |col_id: String, col_index: usize, is_tree_column: bool| CellContext {
            row_index: row,
            col_id,
            col_index,
            is_selected: false,
            is_focused: false,
            is_hovered: false,
            is_editing: false,
            depth: Some(meta.depth),
            is_tree_column,
        };

        if let Some(full_width) = self.full_width.as_ref().filter(|fw| (fw.at)(row))
            && let Some(tree_col) = tree_col
        {
            let band_ctx = context(
                self.columns[tree_col].spec.id.clone(),
                self.tree_display_pos,
                true,
            );
            let content = self
                .source
                .with_row(row, &|item, _meta| (full_width.delegate)(item, &band_ctx))?;
            let content = ctx.add_boxed(content);
            let chrome = tree_chrome(
                ctx,
                content,
                meta.depth,
                self.indent_per_level,
                meta.has_children,
                meta.is_expanded,
                None,
            );
            let band = ctx.add(BandInset::new(
                chrome,
                self.column_widths.clone(),
                self.pane_boundaries,
                self.tree_display_pos,
            ));
            return Some(ctx.add(FullWidthRow::new(band, Some(height))));
        }

        let mut cells = Vec::with_capacity(self.display_indices.len());
        for (display_pos, &col_idx) in self.display_indices.iter().enumerate() {
            let col = &self.columns[col_idx];
            let is_tree_column = display_pos == self.tree_display_pos;
            let cell_ctx = context(col.spec.id.clone(), display_pos, is_tree_column);
            let widget = self
                .source
                .with_row(row, &|item, _meta| (col.cell)(item, &cell_ctx))?;
            let id = ctx.add_boxed(widget);
            cells.push(if is_tree_column {
                tree_chrome(
                    ctx,
                    id,
                    meta.depth,
                    self.indent_per_level,
                    meta.has_children,
                    meta.is_expanded,
                    None,
                )
            } else {
                id
            });
        }
        Some(
            ctx.add(
                BodyRow::new(
                    cells,
                    row + 2,
                    false,
                    Some(height),
                    self.column_widths.clone(),
                    self.pane_boundaries,
                    self.scroll_x.clone(),
                )
                .a11y_hidden(),
            ),
        )
    }
}

/// The stack of pinned copies, laid over the top of the rows by the
/// `TreeTableView` root.
///
/// Hidden from assistive technology, subtree included: each copy repeats a
/// row that is in the tree already, and reading it a second time — at a place
/// in the reading order where that row is not — would only mislead.
pub(crate) struct PinnedStack<T: 'static> {
    /// The rows to copy, outermost first, written by the root's
    /// `place_children`.
    pub(crate) rows: Signal<Vec<usize>>,
    /// The rows this build copied. The root and `place_children` show line
    /// `k` only while it holds the row the root names for it: a new row lands
    /// one frame after the scroll that named it, and for that frame the line
    /// would show the row it held before.
    pub(crate) built: Rc<RefCell<Vec<usize>>>,
    /// The root's latest [`stack_at`], read by `place_children`.
    pub(crate) layout: Rc<RefCell<Vec<StackLine>>>,
    pub(crate) copies: RowCopies<T>,
    /// What a press on a line does with its row: select it and reveal it.
    pub(crate) on_pick: Rc<dyn Fn(usize, &mut EventContext)>,
    /// The line widgets, outermost first.
    pub(crate) lines: Vec<WidgetId>,
}

impl<T: 'static> std::fmt::Debug for PinnedStack<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PinnedStack")
            .field("built", &self.built.borrow())
            .finish_non_exhaustive()
    }
}

impl<T: 'static> Widget for PinnedStack<T> {
    fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
        self.rows
            .bind_to(ctx.self_id(), ctx.binding_registry(), BindingLevel::Rebuild);
        self.lines.clear();
        let mut built = Vec::new();
        // Re-checked rather than trusted: the rows were named before whatever
        // rebuilt the view, and a model change may have taken some away.
        let count = self.copies.source.visible_count();
        for row in self.rows.get() {
            if row >= count {
                break;
            }
            let Some(copy) = self.copies.build(ctx, row) else {
                break;
            };
            let line = ctx.add(PinnedLine { child: copy });
            // A copy, not a stop: the tree table stays one Tab stop.
            ctx.set_tab_stop(line, false);
            // Anchored: rows above may come and go before the press lands.
            let anchor = self.copies.source.anchor(row);
            let pick = self.on_pick.clone();
            ctx.apply_handlers(
                line,
                HandlerSet::new().on_tap(move |_tap, ctx| {
                    if let Some(index) = anchor.index() {
                        pick(index, ctx);
                    }
                }),
            );
            built.push(row);
            self.lines.push(line);
        }
        *self.built.borrow_mut() = built;
        self.children()
    }

    fn layout_response(
        &self,
        proposal: SizeProposal,
        _ctx: &LayoutContext,
    ) -> teksilo_core::widget::LayoutResponse {
        proposal.resolve(0.0, 0.0).into()
    }

    fn place_children(
        &self,
        bounds: Rect,
        _proposal: SizeProposal,
        children: &mut [WidgetPlacement],
        _ctx: &LayoutContext,
    ) {
        let layout = self.layout.borrow();
        let built = self.built.borrow();
        let n = children.len();
        // Children run deepest first (see `children`), lines outermost first.
        for (i, child) in children.iter_mut().enumerate() {
            let k = n - 1 - i;
            match layout.get(k).filter(|line| built.get(k) == Some(&line.row)) {
                Some(line) => {
                    child.origin = Point::new(bounds.x, bounds.y + line.top);
                    child.size = Size::new(bounds.width, line.height);
                }
                None => {
                    child.origin = bounds.origin();
                    child.size = Size::ZERO;
                }
            }
        }
    }

    fn accessibility(&self, builder: &mut AccessNodeBuilder) {
        builder.set_hidden();
    }

    /// Deepest first, so a line pushed up slides *under* the one above it,
    /// which paints — and takes the press — on top.
    fn children(&self) -> Vec<WidgetId> {
        self.lines.iter().rev().copied().collect()
    }

    fn clips_children(&self) -> bool {
        true
    }

    fn as_any(&self) -> Option<&dyn std::any::Any> {
        Some(self)
    }
}

/// One pinned line: a row's copy on an opaque surface, so the rows scrolling
/// beneath do not show through.
///
/// It refuses the hit to everything inside it. The copy is drawn from the
/// row's own delegates, which may hold a control — a checkbox column — and a
/// press on a picture of one must neither toggle it nor take focus: it means
/// the row, and the line answers it.
#[derive(Debug)]
pub(crate) struct PinnedLine {
    child: WidgetId,
}

impl Widget for PinnedLine {
    fn layout_response(
        &self,
        proposal: SizeProposal,
        _ctx: &LayoutContext,
    ) -> teksilo_core::widget::LayoutResponse {
        proposal.resolve(0.0, 0.0).into()
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

    fn paint(&self, bounds: Rect, canvas: &mut Canvas, ctx: &PaintContext) {
        if bounds.height > 0.5 {
            // The header's surface, as `ListView`'s pinned header uses.
            canvas.fill_rect(bounds, SurfaceRole::Raised.resolve(&ctx.theme.colors));
        }
    }

    fn accepts_child_hit(&self, _child: WidgetId, _point: Point) -> bool {
        false
    }

    fn accessibility(&self, builder: &mut AccessNodeBuilder) {
        builder.set_hidden();
    }

    fn children(&self) -> Vec<WidgetId> {
        vec![self.child]
    }
}
