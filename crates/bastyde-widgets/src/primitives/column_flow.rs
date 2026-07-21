// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! `ColumnFlow` — flows children into as many columns as the width affords,
//! re-partitioning every child when a column is gained or lost.
//!
//! The newspaper / CSS multi-column model: content runs down column 0, then
//! down column 1, and so on. The column count is derived from the available
//! width and [`min_column_width`](ColumnFlow::min_column_width) — when the
//! width no longer affords *N* columns the layout drops to *N−1* and **all**
//! children are re-partitioned across the survivors. Children are atomic: one
//! child never straddles a column boundary.
//!
//! Pair it with a [`ScrollArea`](crate::scroll_area::ScrollArea) for vertical
//! overflow — `ColumnFlow` reports its true content height (the tallest
//! column), so the scroll extent is correct.
//!
//! ```rust
//! # use bastyde_widgets::primitives::column_flow::ColumnFlow;
//! # use bastyde_widgets::primitives::TextWidget;
//! # use bastyde_widgets::scroll_area::ScrollArea;
//! # use bastyde_i18n::lit;
//! let _view = ScrollArea::new().child(
//!     ColumnFlow::new()
//!         .min_column_width(240.0)
//!         .max_columns(4)
//!         .column_spacing(16.0)
//!         .item_spacing(12.0)
//!         .child(TextWidget::new(lit!("First")))
//!         .child(TextWidget::new(lit!("Second")))
//!         .child(TextWidget::new(lit!("Third"))),
//! );
//! ```
//!
//! # Reading order
//!
//! Children are distributed as **contiguous runs in source order** — column 0
//! takes children `0..i`, column 1 takes `i..j`. So source order, visual
//! reading order, and focus order are the same thing, at every column count.
//! This is why `ColumnFlow` does not reuse
//! [`MasonryLayout`](crate::primitives::MasonryLayout)'s shortest-column
//! packing, which interleaves children and would divorce the visual order from
//! the source order.
//!
//! # Accessibility
//!
//! By default `ColumnFlow` emits a bare `Role::GenericContainer` carrying no
//! properties, which the accessibility walker *prunes*, promoting the children
//! to its parent in source order. That is the correct outcome for a layout
//! primitive: it contributes geometry, not semantics, and the reading order is
//! already right. Add semantics from the outside with `.access_role(..)` /
//! `.access_label(..)`, or opt into list semantics with
//! [`semantic_list`](ColumnFlow::semantic_list).
//!
//! # Relationship to CSS multi-column
//!
//! Close, but not identical. CSS `column-fill: balance` balances content within
//! a column height it computes from a *bounded* block size; `ColumnFlow` derives
//! the column *count* from the width and lets the height run free (a
//! `ScrollArea` absorbs it). No CSS `column-fill` mode does that, so don't read
//! this as a CSS multicol port.

use std::cell::Cell;

use bastyde_canvas::{Canvas, EdgeInsets, Point, Rect, Size, SizeProposal, StrokeStyle};
use bastyde_core::accessibility::AccessNodeBuilder;
use bastyde_core::binding::BindingLevel;
use bastyde_core::color_prop::ColorProp;
use bastyde_core::signal::{Prop, Signal};
use bastyde_core::widget::{
    LayoutContext, LayoutResponse, PaintContext, PendingChild, Widget, WidgetPlacement,
};
use bastyde_core::widget_id::WidgetId;
use bastyde_tokens::HAlignment;

use crate::common::column_geometry::{ColumnGeometry, WidthPolicy};

/// Default minimum column width, in logical pixels — a card-ish column that
/// reads well at typical desktop sizes.
const DEFAULT_MIN_COLUMN_WIDTH: f32 = 240.0;

/// Bisection steps used by [`balance_columns`].
///
/// A **fixed** count, deliberately, rather than an epsilon-driven `while`
/// loop: `layout_response` and `place_children` each run the search
/// independently (there is no persisted partition state — the
/// `MasonryLayout` pattern), and only an identical, input-independent
/// iteration count makes both calls return bit-identical results. An epsilon
/// loop would iterate a different number of times for different inputs and
/// could settle either side of a boundary, letting the reported height
/// disagree with the placed one.
///
/// 48 halvings drive the interval below any representable `f32` gap over the
/// ranges layout deals in.
const BISECTION_STEPS: u32 = 48;

/// The result of partitioning children into columns.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct BalanceResult {
    /// The tallest column's extent — the container's content height.
    pub height: f32,
    /// `column_of[i]` is the column index child `i` was placed in.
    pub column_of: Vec<usize>,
}

/// The extent of a column holding `count` items totalling `sum`, including the
/// `count - 1` inter-item gaps.
#[inline]
fn run_extent(sum: f32, count: usize, gap: f32) -> f32 {
    if count == 0 {
        0.0
    } else {
        sum + (count as f32 - 1.0) * gap
    }
}

/// How many columns a greedy left-to-right fill needs if no column may exceed
/// `limit`. The feasibility oracle for [`balance_columns`]'s bisection.
///
/// Counts items rather than testing `accumulated > 0.0` to decide whether a
/// gap applies: a run of zero-height children is still a run of *n* items with
/// *n−1* gaps between them, and an accumulator test would silently drop those
/// gaps.
fn columns_needed(heights: &[f32], gap: f32, limit: f32) -> usize {
    let mut columns = 1usize;
    let mut count = 0usize;
    let mut sum = 0.0_f32;
    for &h in heights {
        let (next_count, next_sum) = (count + 1, sum + h);
        if count > 0 && run_extent(next_sum, next_count, gap) > limit {
            columns += 1;
            count = 1;
            sum = h;
        } else {
            count = next_count;
            sum = next_sum;
        }
    }
    columns
}

/// Partition `heights` into at most `k` columns as contiguous, source-order
/// runs, minimising the tallest column.
///
/// Bisects the column extent: `columns_needed` is monotone in the limit (a
/// taller limit never needs more columns), so the smallest feasible extent can
/// be found by halving. The lower bound is the tallest single item — no column
/// can be feasible below it, since children are atomic — and the upper bound is
/// every item in one column, gaps included.
///
/// Construction then re-runs the greedy fill at that extent with one extra
/// rule: column `j` may not take so many items that fewer than one remains for
/// each column after it. That is what makes `[10, 10, 10, 10]` into 3 columns
/// come out as `[20, 10, 10]` rather than `[20, 20, ∅]` — both have the same
/// (optimal) tallest column, but the second wastes a column. Reserving items
/// can never force a column past the limit; it only ever makes a column take
/// *fewer* items.
pub(crate) fn balance_columns(heights: &[f32], gap: f32, k: usize) -> BalanceResult {
    let n = heights.len();
    if n == 0 {
        return BalanceResult {
            height: 0.0,
            column_of: Vec::new(),
        };
    }
    let gap = gap.max(0.0);
    // More columns than items would leave trailing columns unavoidably empty.
    let k_eff = k.min(n).max(1);

    // Bisect for the smallest feasible column extent.
    let mut lo = heights.iter().copied().fold(0.0_f32, f32::max).max(0.0);
    let mut hi = heights.iter().copied().sum::<f32>() + (n as f32 - 1.0).max(0.0) * gap;
    if hi < lo {
        hi = lo;
    }
    for _ in 0..BISECTION_STEPS {
        let mid = lo + (hi - lo) * 0.5;
        if columns_needed(heights, gap, mid) <= k_eff {
            hi = mid;
        } else {
            lo = mid;
        }
    }
    // `hi` is always feasible; `lo` may not be. Never report `lo`.
    let limit = hi;

    // Construct at `limit`, reserving at least one item per remaining column.
    let mut column_of = vec![0usize; n];
    let mut placed = 0usize;
    let mut idx = 0usize;
    for col in 0..k_eff {
        let remaining = n - placed;
        let reserve = k_eff - col - 1;
        let cap = if col + 1 == k_eff {
            remaining
        } else {
            remaining.saturating_sub(reserve)
        }
        .max(1);

        let mut count = 0usize;
        let mut sum = 0.0_f32;
        while count < cap && idx < n {
            let (next_count, next_sum) = (count + 1, sum + heights[idx]);
            if count > 0 && run_extent(next_sum, next_count, gap) > limit {
                break;
            }
            column_of[idx] = col;
            count = next_count;
            sum = next_sum;
            idx += 1;
        }
        placed += count;
    }
    // Defensive: if the reserve rule ever stranded a tail (it should not), put
    // it in the last column rather than dropping it on the floor.
    for slot in column_of.iter_mut().skip(idx) {
        *slot = k_eff - 1;
    }

    let height = (0..k_eff)
        .map(|c| {
            let mut count = 0usize;
            let mut sum = 0.0_f32;
            for (i, &h) in heights.iter().enumerate() {
                if column_of[i] == c {
                    count += 1;
                    sum += h;
                }
            }
            run_extent(sum, count, gap)
        })
        .fold(0.0_f32, f32::max);

    BalanceResult { height, column_of }
}

/// A layout that flows its children into as many columns as the available
/// width affords, re-partitioning every child when a column is gained or lost.
///
/// ```text
///  wide                            narrower
/// ┌────┐ ┌────┐ ┌────┐            ┌────┐ ┌────┐
/// │ 1  │ │ 3  │ │ 5  │            │ 1  │ │ 4  │
/// ├────┤ ├────┤ ├────┤            ├────┤ ├────┤
/// │ 2  │ │ 4  │ │ 6  │    ───►    │ 2  │ │ 5  │
/// └────┘ └────┘ └────┘            ├────┤ ├────┤
///                                 │ 3  │ │ 6  │
///                                 └────┘ └────┘
/// ```
///
/// Reading order is 1..6 at both widths. See the [module docs](self).
pub struct ColumnFlow {
    min_column_width: f32,
    max_column_width: Option<f32>,
    max_columns: Option<usize>,
    column_spacing: Prop<f32>,
    item_spacing: Prop<f32>,
    alignment: HAlignment,
    column_rule: Option<(f32, ColorProp)>,
    semantic_list: bool,
    child_ids: Vec<WidgetId>,
    pending: Vec<PendingChild>,
    /// Published column count. Written from `place_children` behind
    /// `last_count`; see [`column_count_signal`](Self::column_count_signal).
    column_count: Signal<usize>,
    last_count: Cell<usize>,
}

impl std::fmt::Debug for ColumnFlow {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ColumnFlow")
            .field("min_column_width", &self.min_column_width)
            .field("max_column_width", &self.max_column_width)
            .field("max_columns", &self.max_columns)
            .field("alignment", &self.alignment)
            .field("semantic_list", &self.semantic_list)
            .field("children", &self.child_ids.len())
            .field("column_count", &self.last_count.get())
            .finish()
    }
}

impl ColumnFlow {
    /// Create a `ColumnFlow` with a 240 dp minimum column width, no maximum
    /// column width, and no column-count cap.
    pub fn new() -> Self {
        Self {
            min_column_width: DEFAULT_MIN_COLUMN_WIDTH,
            max_column_width: None,
            max_columns: None,
            column_spacing: Prop::Static(0.0),
            item_spacing: Prop::Static(0.0),
            alignment: HAlignment::Leading,
            column_rule: None,
            semantic_list: false,
            child_ids: Vec::new(),
            pending: Vec::new(),
            column_count: Signal::new(1),
            last_count: Cell::new(1),
        }
    }

    /// The narrowest a column may be. The column count is the largest *N* whose
    /// columns are all at least this wide — CSS `column-width` / SwiftUI
    /// `GridItem(.adaptive(minimum:))` / Compose `GridCells.Adaptive(minSize)`.
    ///
    /// A value of zero or less pins the layout to a single column.
    pub fn min_column_width(mut self, width: f32) -> Self {
        self.min_column_width = width;
        self
    }

    /// The widest a column may be. Unset by default, so columns stretch to
    /// share the full width evenly.
    ///
    /// Set it to stop columns becoming unreadably wide when few of them fit a
    /// large display — the reason KDE's `Kirigami.CardsLayout` pairs
    /// `minimumColumnWidth` with `maximumColumnWidth`. When it bites, the
    /// columns no longer fill the width and
    /// [`alignment`](Self::alignment) decides where the block sits.
    pub fn max_column_width(mut self, width: f32) -> Self {
        self.max_column_width = Some(width);
        self
    }

    /// Never use more than `max` columns however wide the layout gets.
    ///
    /// Also decides the count when the width is unconstrained (inside a
    /// size-to-content parent such as a popover): unset, that case reports one
    /// column, matching CSS `column-count: auto` in a shrink-to-fit context.
    /// Clamped to at least 1.
    pub fn max_columns(mut self, max: usize) -> Self {
        self.max_columns = Some(max.max(1));
        self
    }

    /// Horizontal gap between columns. Accepts an `f32` or a `Signal<f32>`.
    pub fn column_spacing(mut self, spacing: impl Into<Prop<f32>>) -> Self {
        self.column_spacing = spacing.into();
        self
    }

    /// Vertical gap between items within a column. Accepts an `f32` or a
    /// `Signal<f32>`.
    ///
    /// Named for items rather than rows because there are no rows here: a
    /// column's items are independent of its neighbours'.
    pub fn item_spacing(mut self, spacing: impl Into<Prop<f32>>) -> Self {
        self.item_spacing = spacing.into();
        self
    }

    /// Where the column block sits when it does not fill the available width.
    ///
    /// Only observable once [`max_column_width`](Self::max_column_width) clamps
    /// the columns narrower than their even share — otherwise the columns
    /// consume the whole width and there is nothing to align. Defaults to
    /// [`HAlignment::Leading`]; RTL-aware.
    pub fn alignment(mut self, alignment: HAlignment) -> Self {
        self.alignment = alignment;
        self
    }

    /// Draw a rule of `width` dp, centred in every inter-column gap — CSS
    /// `column-rule`.
    ///
    /// Purely decorative: it emits no accessibility node. Accepts a `Color`, a
    /// theme role, or a `Signal`. Pass `BorderRole::Divider` to track the
    /// theme's divider colour.
    pub fn column_rule(mut self, width: f32, color: impl Into<ColorProp>) -> Self {
        self.column_rule = Some((width, color.into()));
        self
    }

    /// Expose the children to assistive technology as a list.
    ///
    /// The container becomes `Role::List` and every child is wrapped in a
    /// layout-transparent node reporting `Role::ListItem` with its position and
    /// the set size, so a screen reader announces "list, 30 items" and
    /// "item 5 of 30" rather than reading 30 unrelated widgets.
    ///
    /// Off by default: a layout primitive should not invent semantics its
    /// content may not have. Turn it on when the children genuinely *are* a
    /// list of peers. Costs one extra node per child.
    pub fn semantic_list(mut self, enabled: bool) -> Self {
        self.semantic_list = enabled;
        self
    }

    /// Add a pre-registered child by ID.
    pub fn add_child(mut self, id: WidgetId) -> Self {
        self.pending.push(PendingChild::Id(id));
        self
    }

    /// Add an inline child widget (deferred insertion).
    pub fn child(mut self, widget: impl Widget + 'static) -> Self {
        self.pending.push(PendingChild::Deferred(Box::new(widget)));
        self
    }

    /// Add multiple inline children from an iterator.
    pub fn children(mut self, iter: impl IntoIterator<Item = impl Widget + 'static>) -> Self {
        for widget in iter {
            self.pending.push(PendingChild::Deferred(Box::new(widget)));
        }
        self
    }

    /// Conditionally add a child. No-op if `None`.
    pub fn child_opt(mut self, widget: Option<impl Widget + 'static>) -> Self {
        if let Some(w) = widget {
            self.pending.push(PendingChild::Deferred(Box::new(w)));
        }
        self
    }

    /// The live column count, as a reactive signal.
    ///
    /// Lets an app follow the reflow — swapping to a compact header at one
    /// column, say. Written from the layout pass behind an equality guard, so
    /// it only fires when the count actually changes.
    ///
    /// **Binding contract.** Safe for `RepaintOnly` / `AccessibilityOnly`
    /// consumers, and for `Relayout` consumers that do not feed back into this
    /// widget's own width. The count is a pure function of the width
    /// `ColumnFlow` is *given* — it never changes its own width, so it cannot
    /// oscillate on its own. But a `Relayout` consumer that resizes something
    /// which in turn resizes this `ColumnFlow` closes a feedback loop through
    /// the layout pass, which is exactly what
    /// [`Widget::place_children`]'s own documentation warns against.
    ///
    /// [`Widget::place_children`]: bastyde_core::widget::Widget::place_children
    pub fn column_count_signal(&self) -> Signal<usize> {
        self.column_count.clone()
    }

    /// The column-sizing policy, as understood by the shared solver.
    fn width_policy(&self) -> WidthPolicy {
        WidthPolicy::Adaptive {
            min: self.min_column_width,
            max: self.max_column_width,
        }
    }

    /// The solver for a given inter-column gap. `ColumnFlow` carries no insets
    /// (wrap it in `Padding`), and does its own x placement so it can align the
    /// block and mirror for RTL.
    ///
    /// `max_columns` goes *into* the solver rather than clamping its result:
    /// `column_width` divides the width by the count, so a cap applied
    /// afterwards would size columns for the uncapped count.
    fn geometry(&self, col_spacing: f32) -> ColumnGeometry {
        ColumnGeometry::from_policy(self.width_policy(), col_spacing, EdgeInsets::ZERO)
            .with_max_columns(self.max_columns)
    }

    /// Column count at `width`, honouring [`max_columns`](Self::max_columns).
    fn column_count_at(&self, width: f32, col_spacing: f32) -> usize {
        self.geometry(col_spacing).column_count(width)
    }

    /// Measure every active child at `col_width`, in source order.
    ///
    /// Returns the ids alongside their heights: `child_size` yields `None` for
    /// dormant children, which is exactly the subset `place_children` receives,
    /// so both hooks agree on which children exist without extra bookkeeping.
    fn measure(
        &self,
        ids: &[WidgetId],
        col_width: f32,
        ctx: &LayoutContext,
    ) -> (Vec<WidgetId>, Vec<f32>) {
        let proposal = SizeProposal::with_width(col_width);
        let mut live = Vec::with_capacity(ids.len());
        let mut heights = Vec::with_capacity(ids.len());
        for &id in ids {
            if let Some(size) = ctx.child_size(id, proposal) {
                live.push(id);
                heights.push(size.height);
            }
        }
        (live, heights)
    }

    /// The width a column should take when the parent constrains nothing.
    fn intrinsic_column_width(&self, ids: &[WidgetId], ctx: &LayoutContext) -> f32 {
        let mut widest = 0.0_f32;
        for &id in ids {
            if let Some(size) = ctx.child_size(id, SizeProposal::unspecified()) {
                widest = widest.max(size.width);
            }
        }
        let mut w = widest.max(self.min_column_width);
        if let Some(max) = self.max_column_width {
            w = w.min(max);
        }
        w.max(0.0)
    }
}

impl Default for ColumnFlow {
    fn default() -> Self {
        Self::new()
    }
}

impl Widget for ColumnFlow {
    fn build(&mut self, ctx: &mut bastyde_core::build_context::BuildContext) -> Vec<WidgetId> {
        let pending = std::mem::take(&mut self.pending);
        if !pending.is_empty() {
            let resolved: Vec<WidgetId> = pending
                .into_iter()
                .map(|child| match child {
                    PendingChild::Id(id) => id,
                    PendingChild::Deferred(w) => ctx.add_boxed(w),
                })
                .collect();

            self.child_ids = if self.semantic_list {
                // Wrap each child so it can carry Role::ListItem + its position.
                let total = resolved.len();
                resolved
                    .into_iter()
                    .enumerate()
                    .map(|(i, id)| ctx.add(ColumnFlowItem::new(id, i + 1, total)))
                    .collect()
            } else {
                resolved
            };
        }

        let self_id = ctx.self_id();
        let registry = ctx.binding_registry();
        self.column_spacing
            .register_if_bound(self_id, registry, BindingLevel::Relayout);
        self.item_spacing
            .register_if_bound(self_id, registry, BindingLevel::Relayout);

        self.child_ids.clone()
    }

    fn layout_response(&self, proposal: SizeProposal, ctx: &LayoutContext) -> LayoutResponse {
        if self.child_ids.is_empty() {
            return proposal.resolve(0.0, 0.0).into();
        }

        let col_spacing = self.column_spacing.get();
        let item_spacing = self.item_spacing.get();

        let (total_width, columns, col_width) = match proposal.width {
            Some(w) => (
                // Echo the proposal verbatim. Recomputing the width from the
                // resolved columns would feed a slightly different value back
                // into `column_count_at` in `place_children` (which reads
                // `bounds.width`), and a single ULP could flip a column.
                w,
                self.column_count_at(w, col_spacing),
                self.geometry(col_spacing).column_width(w),
            ),
            None => {
                // Unconstrained: a size-to-content parent (popover, menu) takes
                // this answer verbatim, so it must be finite and modest.
                let columns = self.max_columns.unwrap_or(1).max(1);
                let col_width = self.intrinsic_column_width(&self.child_ids, ctx);
                let gaps = col_spacing.max(0.0) * (columns as f32 - 1.0).max(0.0);
                (col_width * columns as f32 + gaps, columns, col_width)
            }
        };

        let (_, heights) = self.measure(&self.child_ids, col_width, ctx);
        let balance = balance_columns(&heights, item_spacing, columns);
        Size::new(total_width, balance.height).into()
    }

    fn place_children(
        &self,
        bounds: Rect,
        _proposal: SizeProposal,
        children: &mut [WidgetPlacement],
        ctx: &LayoutContext,
    ) {
        let col_spacing = self.column_spacing.get().max(0.0);
        let item_spacing = self.item_spacing.get();

        // Derive from the actual bounds, not the proposal — `bounds.width` is
        // what `layout_response` echoed back, so both agree.
        let columns = self.column_count_at(bounds.width, col_spacing);
        self.publish_column_count(columns);

        if children.is_empty() {
            return;
        }

        let geometry = self.geometry(col_spacing);
        let col_width = geometry.column_width(bounds.width);
        let used = geometry.used_width(bounds.width).min(bounds.width);
        let rtl = ctx.is_rtl();
        let block_x = bounds.x + self.alignment.resolve(used, bounds.width, rtl);

        let ids: Vec<WidgetId> = children.iter().map(|c| c.id).collect();
        let (_, heights) = self.measure(&ids, col_width, ctx);
        if heights.len() != ids.len() {
            // `children` is already the active subset, so every id must
            // measure. Bail rather than misplace them if that ever changes.
            return;
        }
        let balance = balance_columns(&heights, item_spacing, columns);

        let mut col_y = vec![bounds.y; columns.max(1)];
        for (i, child) in children.iter_mut().enumerate() {
            let col = balance.column_of[i].min(columns.saturating_sub(1));
            // Logical column 0 sits at the leading edge in both directions.
            let physical = if rtl { columns - 1 - col } else { col };
            let x = block_x + physical as f32 * (col_width + col_spacing);

            if col_y[col] > bounds.y {
                col_y[col] += item_spacing;
            }
            child.origin = Point::new(x, col_y[col]);
            child.size = Size::new(col_width, heights[i]);
            col_y[col] += heights[i];
        }
    }

    fn paint(&self, bounds: Rect, canvas: &mut Canvas, ctx: &PaintContext) {
        let Some((rule_width, ref color)) = self.column_rule else {
            return;
        };
        if rule_width <= 0.0 {
            return;
        }
        let col_spacing = self.column_spacing.get().max(0.0);
        let columns = self.column_count_at(bounds.width, col_spacing);
        if columns < 2 {
            return;
        }

        let geometry = self.geometry(col_spacing);
        let col_width = geometry.column_width(bounds.width);
        let used = geometry.used_width(bounds.width).min(bounds.width);
        let rtl = ctx.layout_direction == bastyde_core::environment::LayoutDirection::RightToLeft;
        let block_x = bounds.x + self.alignment.resolve(used, bounds.width, rtl);
        let resolved = color.resolve(ctx.theme, ctx.effective_enabled);

        // One rule centred in each of the `columns - 1` gaps. Gap positions are
        // symmetric, so no RTL mirroring is needed here.
        for gap_index in 0..columns - 1 {
            let x = block_x
                + (gap_index as f32 + 1.0) * col_width
                + gap_index as f32 * col_spacing
                + col_spacing / 2.0;
            canvas.draw_line(
                Point::new(x, bounds.y),
                Point::new(x, bounds.bottom()),
                resolved,
                StrokeStyle::solid(rule_width),
            );
        }
    }

    fn accessibility(&self, builder: &mut AccessNodeBuilder) {
        if self.semantic_list {
            builder.set_role(bastyde_core::accesskit::Role::List);
        } else {
            // Deliberately bare: the walker prunes a property-free
            // GenericContainer and promotes the children in source order,
            // which is already the reading order. Setting anything here (even
            // an orientation) would keep this node alive as AT noise.
            builder.set_role(bastyde_core::accesskit::Role::GenericContainer);
        }
    }

    fn children(&self) -> Vec<WidgetId> {
        self.child_ids.clone()
    }
}

impl ColumnFlow {
    /// Publish the column count, guarded so the signal only fires on a real
    /// change. The guard is what keeps a `Relayout`-bound consumer from
    /// re-dirtying the tree on every pass.
    fn publish_column_count(&self, columns: usize) {
        if self.last_count.get() != columns {
            self.last_count.set(columns);
            self.column_count.set(columns);
        }
    }
}

/// Layout-transparent wrapper giving one `ColumnFlow` child its list-item
/// accessibility identity. Mounted only under
/// [`ColumnFlow::semantic_list`].
///
/// Mirrors `ListItemWrapper` in [`crate::list_item_a11y`], including its
/// flatten-to-`Size` layout: `ColumnFlow` reads only `.size` off its children,
/// so there is no grow/shrink weight for this wrapper to forward.
#[derive(Debug)]
struct ColumnFlowItem {
    child: WidgetId,
    /// 1-based.
    position: usize,
    total: usize,
}

impl ColumnFlowItem {
    fn new(child: WidgetId, position_1based: usize, total: usize) -> Self {
        Self {
            child,
            position: position_1based,
            total,
        }
    }
}

impl Widget for ColumnFlowItem {
    fn layout_response(&self, proposal: SizeProposal, ctx: &LayoutContext) -> LayoutResponse {
        ctx.child_size(self.child, proposal)
            .unwrap_or_else(|| proposal.resolve(0.0, 0.0))
            .into()
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
        builder.set_role(bastyde_core::accesskit::Role::ListItem);
        builder.set_position_in_set(self.position);
        builder.set_size_of_set(self.total);
    }

    fn children(&self) -> Vec<WidgetId> {
        vec![self.child]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bastyde_core::widget_tree::WidgetTree;

    // ── balance_columns ──────────────────────────────────────────────

    /// Reconstruct each column's extent from a partition, so tests assert
    /// against the geometry rather than the algorithm's own arithmetic.
    fn column_extents(heights: &[f32], gap: f32, r: &BalanceResult) -> Vec<f32> {
        let cols = r.column_of.iter().copied().max().map_or(0, |m| m + 1);
        (0..cols)
            .map(|c| {
                let (mut count, mut sum) = (0usize, 0.0_f32);
                for (i, &h) in heights.iter().enumerate() {
                    if r.column_of[i] == c {
                        count += 1;
                        sum += h;
                    }
                }
                run_extent(sum, count, gap)
            })
            .collect()
    }

    #[test]
    fn uses_every_column_instead_of_stranding_a_trailing_one() {
        // The empty-trailing-column regression. Naive min-max would pack
        // [10,10] [10,10] [] — same tallest column, one column wasted.
        let h = [10.0, 10.0, 10.0, 10.0];
        let r = balance_columns(&h, 0.0, 3);
        assert_eq!(r.column_of, vec![0, 0, 1, 2]);
        assert_eq!(column_extents(&h, 0.0, &r), vec![20.0, 10.0, 10.0]);
        assert!((r.height - 20.0).abs() < 0.01);
    }

    #[test]
    fn evenly_divisible_input_splits_evenly() {
        let h = [10.0; 9];
        let r = balance_columns(&h, 0.0, 3);
        assert_eq!(r.column_of, vec![0, 0, 0, 1, 1, 1, 2, 2, 2]);
        assert!((r.height - 30.0).abs() < 0.01);
    }

    #[test]
    fn single_column_extent_includes_every_gap() {
        // 3 items of 10 with gap 5 in one column = 30 + 2*5 = 40. If the
        // bisection's upper bound omitted the gaps it would cap at 30.
        let h = [10.0, 10.0, 10.0];
        let r = balance_columns(&h, 5.0, 1);
        assert_eq!(r.column_of, vec![0, 0, 0]);
        assert!((r.height - 40.0).abs() < 0.01, "height was {}", r.height);
    }

    #[test]
    fn zero_height_items_still_pay_the_gap() {
        // The count-not-accumulator regression: an `if accum > 0.0` gap test
        // reports 0.0 here, because the running sum never leaves zero.
        let h = [0.0, 0.0, 0.0, 0.0];
        let r = balance_columns(&h, 8.0, 2);
        assert!(
            (r.height - 8.0).abs() < 0.01,
            "two zero-height items in a column still span one gap, got {}",
            r.height
        );
    }

    #[test]
    fn more_columns_than_items_does_not_panic() {
        let h = [10.0, 20.0];
        let r = balance_columns(&h, 0.0, 5);
        assert_eq!(r.column_of, vec![0, 1], "clamped to one column per item");
        assert!((r.height - 20.0).abs() < 0.01);
    }

    #[test]
    fn empty_input_is_zero() {
        let r = balance_columns(&[], 4.0, 3);
        assert!(r.column_of.is_empty());
        assert_eq!(r.height, 0.0);
    }

    #[test]
    fn single_item() {
        let r = balance_columns(&[50.0], 0.0, 3);
        assert_eq!(r.column_of, vec![0]);
        assert!((r.height - 50.0).abs() < 0.01);
    }

    #[test]
    fn one_giant_item_sets_the_floor() {
        // No column can be shorter than the tallest atomic child.
        let h = [200.0, 10.0, 10.0, 10.0];
        let r = balance_columns(&h, 0.0, 3);
        assert!(r.height >= 200.0 - 0.01, "height was {}", r.height);
        assert_eq!(r.column_of[0], 0);
    }

    #[test]
    fn negative_gap_is_clamped() {
        let h = [10.0, 10.0];
        let r = balance_columns(&h, -100.0, 1);
        assert!((r.height - 20.0).abs() < 0.01, "height was {}", r.height);
    }

    #[test]
    fn partition_is_contiguous_and_ordered() {
        // The a11y keystone: columns are runs, and run k+1 starts after run k.
        let h = [10.0, 10.0, 10.0, 40.0, 10.0, 10.0];
        let r = balance_columns(&h, 0.0, 2);
        for w in r.column_of.windows(2) {
            assert!(
                w[1] >= w[0],
                "column index must never go backwards: {:?}",
                r.column_of
            );
        }
    }

    #[test]
    fn reported_height_matches_reconstructed_columns() {
        // Property-ish: the reported height must equal the tallest column as
        // actually laid out, across a spread of shapes.
        let cases: &[(&[f32], f32, usize)] = &[
            (&[10.0, 10.0, 10.0, 10.0], 0.0, 3),
            (
                &[
                    1.0, 1.0, 1.0, 1.0, 1.0, 1.0, 1.0, 1.0, 1.0, 1.0, 40.0, 1.0, 1.0,
                ],
                4.0,
                5,
            ),
            (&[5.0, 100.0, 5.0], 2.0, 2),
            (&[7.0; 13], 3.0, 4),
            (&[0.0, 5.0, 0.0, 5.0], 1.0, 2),
            (&[33.0, 12.0, 90.0, 4.0, 61.0, 8.0], 6.0, 3),
        ];
        for (h, gap, k) in cases {
            let r = balance_columns(h, *gap, *k);
            let extents = column_extents(h, *gap, &r);
            let tallest = extents.iter().copied().fold(0.0_f32, f32::max);
            assert!(
                (r.height - tallest).abs() < 0.01,
                "reported {} vs reconstructed {} for {:?} gap {} k {}",
                r.height,
                tallest,
                h,
                gap,
                k
            );
            assert_eq!(h.len(), r.column_of.len());
        }
    }

    #[test]
    fn no_column_exceeds_the_reported_height() {
        let h = [
            1.0, 1.0, 1.0, 1.0, 1.0, 1.0, 1.0, 1.0, 1.0, 1.0, 40.0, 1.0, 1.0,
        ];
        let r = balance_columns(&h, 4.0, 5);
        for (c, extent) in column_extents(&h, 4.0, &r).iter().enumerate() {
            assert!(
                *extent <= r.height + 0.01,
                "column {c} extent {extent} exceeds reported {}",
                r.height
            );
        }
    }

    #[test]
    fn is_deterministic_across_repeated_calls() {
        // layout_response and place_children each run the search from scratch;
        // they must agree bit-for-bit.
        let h = [33.0, 12.0, 90.0, 4.0, 61.0, 8.0, 17.0];
        let a = balance_columns(&h, 6.0, 3);
        let b = balance_columns(&h, 6.0, 3);
        assert_eq!(a, b);
    }

    // ── widget ───────────────────────────────────────────────────────

    #[derive(Debug)]
    struct FixedLeaf(f32, f32);
    impl Widget for FixedLeaf {
        fn layout_response(&self, _p: SizeProposal, _c: &LayoutContext) -> LayoutResponse {
            Size::new(self.0, self.1).into()
        }
    }

    /// A leaf that carries real semantics, so the a11y walker keeps it.
    /// `FixedLeaf` emits a bare `Role::Unknown` and is itself presentational —
    /// it would be pruned right along with the container, which proves
    /// nothing about promotion.
    #[derive(Debug)]
    struct LabeledLeaf(f32, f32, &'static str);
    impl Widget for LabeledLeaf {
        fn layout_response(&self, _p: SizeProposal, _c: &LayoutContext) -> LayoutResponse {
            Size::new(self.0, self.1).into()
        }
        fn accessibility(&self, builder: &mut AccessNodeBuilder) {
            builder.set_role(bastyde_core::accesskit::Role::Button);
            builder.set_name(self.2);
        }
    }

    /// Six 40 dp-tall children, `min_column_width` 100.
    fn six_children(tree: &mut WidgetTree) -> (Vec<WidgetId>, WidgetId) {
        let ids: Vec<_> = (0..6).map(|_| tree.add(FixedLeaf(50.0, 40.0))).collect();
        let mut flow = ColumnFlow::new().min_column_width(100.0);
        for &id in &ids {
            flow = flow.add_child(id);
        }
        let flow_id = tree.add(flow);
        (ids, flow_id)
    }

    #[test]
    fn column_count_follows_width() {
        let mut tree = WidgetTree::new();
        let (ids, _) = six_children(&mut tree);

        // 300 wide / min 100 -> 3 columns, 2 items each.
        tree.layout(SizeProposal::exact(300.0, 400.0));
        assert!((tree.bounds(ids[0]).x - 0.0).abs() < 0.01);
        assert!((tree.bounds(ids[2]).x - 100.0).abs() < 0.01);
        assert!((tree.bounds(ids[4]).x - 200.0).abs() < 0.01);
    }

    #[test]
    fn losing_a_column_repartitions_every_child() {
        let mut tree = WidgetTree::new();
        let (ids, _) = six_children(&mut tree);

        // 3 columns: [0,1] [2,3] [4,5]
        tree.layout(SizeProposal::exact(300.0, 400.0));
        assert!((tree.bounds(ids[2]).x - 100.0).abs() < 0.01);
        assert!((tree.bounds(ids[3]).y - 40.0).abs() < 0.01);

        // 2 columns: [0,1,2] [3,4,5] — child 2 moved back to column 0 and
        // child 3 became the top of column 1. Every child was repartitioned.
        tree.layout(SizeProposal::exact(200.0, 400.0));
        assert!(
            (tree.bounds(ids[2]).x - 0.0).abs() < 0.01,
            "child 2 -> col 0"
        );
        assert!((tree.bounds(ids[2]).y - 80.0).abs() < 0.01);
        assert!(
            (tree.bounds(ids[3]).x - 100.0).abs() < 0.01,
            "child 3 -> col 1"
        );
        assert!(
            (tree.bounds(ids[3]).y - 0.0).abs() < 0.01,
            "child 3 tops col 1"
        );

        // 1 column: everything stacks.
        tree.layout(SizeProposal::exact(100.0, 400.0));
        for (i, &id) in ids.iter().enumerate() {
            assert!((tree.bounds(id).x - 0.0).abs() < 0.01);
            assert!((tree.bounds(id).y - (i as f32 * 40.0)).abs() < 0.01);
        }
    }

    #[test]
    fn reported_height_matches_placed_content() {
        // layout_response and place_children must agree — the container must
        // never report a height its own children overflow.
        let mut tree = WidgetTree::new();
        let heights = [30.0, 70.0, 20.0, 55.0, 45.0];
        let ids: Vec<_> = heights
            .iter()
            .map(|&h| tree.add(FixedLeaf(50.0, h)))
            .collect();
        let mut flow = ColumnFlow::new().min_column_width(100.0).item_spacing(8.0);
        for &id in &ids {
            flow = flow.add_child(id);
        }
        let flow_id = tree.add(flow);

        for width in [100.0, 200.0, 300.0, 400.0, 500.0] {
            tree.layout(SizeProposal {
                width: Some(width),
                height: None,
            });
            let reported = tree.bounds(flow_id).height;
            let top = tree.bounds(flow_id).y;
            let deepest = ids
                .iter()
                .map(|&id| tree.bounds(id).bottom() - top)
                .fold(0.0_f32, f32::max);
            assert!(
                (reported - deepest).abs() < 0.01,
                "at width {width}: reported {reported}, content reaches {deepest}"
            );
        }
    }

    #[test]
    fn children_receive_the_column_width() {
        let mut tree = WidgetTree::new();
        let (ids, _) = six_children(&mut tree);
        tree.layout(SizeProposal::exact(300.0, 400.0));
        // Placed at the column width (100), not their intrinsic 50.
        assert!((tree.bounds(ids[0]).width - 100.0).abs() < 0.01);
    }

    #[test]
    fn column_spacing_applied() {
        let mut tree = WidgetTree::new();
        let ids: Vec<_> = (0..4).map(|_| tree.add(FixedLeaf(50.0, 40.0))).collect();
        let mut flow = ColumnFlow::new()
            .min_column_width(100.0)
            .column_spacing(10.0);
        for &id in &ids {
            flow = flow.add_child(id);
        }
        tree.add(flow);
        // floor((320 + 10) / (100 + 10)) = 3 columns; width (320 - 20)/3 = 100.
        tree.layout(SizeProposal::exact(320.0, 400.0));
        assert!((tree.bounds(ids[0]).x - 0.0).abs() < 0.01);
        assert!((tree.bounds(ids[2]).x - 110.0).abs() < 0.01);
        assert!((tree.bounds(ids[3]).x - 220.0).abs() < 0.01);
    }

    #[test]
    fn item_spacing_applied() {
        let mut tree = WidgetTree::new();
        let ids: Vec<_> = (0..4).map(|_| tree.add(FixedLeaf(50.0, 40.0))).collect();
        let mut flow = ColumnFlow::new().min_column_width(100.0).item_spacing(8.0);
        for &id in &ids {
            flow = flow.add_child(id);
        }
        tree.add(flow);
        // 2 columns: [0,1] [2,3]; second item sits at 40 + 8.
        tree.layout(SizeProposal::exact(200.0, 400.0));
        assert!((tree.bounds(ids[1]).y - 48.0).abs() < 0.01);
        assert!((tree.bounds(ids[3]).y - 48.0).abs() < 0.01);
    }

    #[test]
    fn max_columns_caps_the_count() {
        let mut tree = WidgetTree::new();
        let (ids, _) = {
            let ids: Vec<_> = (0..6).map(|_| tree.add(FixedLeaf(50.0, 40.0))).collect();
            let mut flow = ColumnFlow::new().min_column_width(100.0).max_columns(2);
            for &id in &ids {
                flow = flow.add_child(id);
            }
            let flow_id = tree.add(flow);
            (ids, flow_id)
        };
        // 600 wide would fit 6 columns, but max_columns pins it to 2.
        tree.layout(SizeProposal::exact(600.0, 400.0));
        assert!((tree.bounds(ids[0]).x - 0.0).abs() < 0.01);
        assert!((tree.bounds(ids[3]).x - 300.0).abs() < 0.01);
        assert!((tree.bounds(ids[5]).x - 300.0).abs() < 0.01);
    }

    #[test]
    fn max_column_width_clamps_and_alignment_places_the_block() {
        let mut tree = WidgetTree::new();
        let ids: Vec<_> = (0..2).map(|_| tree.add(FixedLeaf(50.0, 40.0))).collect();
        let mut flow = ColumnFlow::new()
            .min_column_width(400.0)
            .max_column_width(300.0)
            .max_columns(2)
            .alignment(HAlignment::Center);
        for &id in &ids {
            flow = flow.add_child(id);
        }
        tree.add(flow);
        // 1000 wide: 2 columns, each clamped 500 -> 300. Used = 600,
        // leftover = 400, centred -> block starts at 200.
        tree.layout(SizeProposal::exact(1000.0, 400.0));
        assert!((tree.bounds(ids[0]).width - 300.0).abs() < 0.01);
        assert!(
            (tree.bounds(ids[0]).x - 200.0).abs() < 0.01,
            "centred block, got x = {}",
            tree.bounds(ids[0]).x
        );
        assert!((tree.bounds(ids[1]).x - 500.0).abs() < 0.01);
    }

    #[test]
    fn unbounded_width_reports_one_column_by_default() {
        let mut tree = WidgetTree::new();
        let a = tree.add(FixedLeaf(80.0, 40.0));
        let b = tree.add(FixedLeaf(60.0, 30.0));
        let flow = tree.add(
            ColumnFlow::new()
                .min_column_width(50.0)
                .add_child(a)
                .add_child(b),
        );
        tree.layout(SizeProposal {
            width: None,
            height: Some(400.0),
        });
        // One column at the widest child (80). A size-to-content parent takes
        // this verbatim, so it must not balloon.
        assert!(
            (tree.bounds(flow).width - 80.0).abs() < 0.01,
            "got {}",
            tree.bounds(flow).width
        );
    }

    #[test]
    fn unbounded_width_honours_max_columns() {
        let mut tree = WidgetTree::new();
        let a = tree.add(FixedLeaf(80.0, 40.0));
        let b = tree.add(FixedLeaf(60.0, 30.0));
        let flow = tree.add(
            ColumnFlow::new()
                .min_column_width(50.0)
                .max_columns(3)
                .column_spacing(10.0)
                .add_child(a)
                .add_child(b),
        );
        tree.layout(SizeProposal {
            width: None,
            height: Some(400.0),
        });
        // 3 columns of 80 + 2 gaps of 10 = 260.
        assert!(
            (tree.bounds(flow).width - 260.0).abs() < 0.01,
            "got {}",
            tree.bounds(flow).width
        );
    }

    #[test]
    fn empty_flow_has_zero_height() {
        let mut tree = WidgetTree::new();
        let flow = tree.add(ColumnFlow::new());
        tree.layout(SizeProposal {
            width: Some(300.0),
            height: None,
        });
        assert!((tree.bounds(flow).height - 0.0).abs() < 0.01);
    }

    #[test]
    fn dormant_child_excluded_and_partition_stays_stable() {
        let mut tree = WidgetTree::new();
        let ids: Vec<_> = (0..4).map(|_| tree.add(FixedLeaf(50.0, 40.0))).collect();
        let mut flow = ColumnFlow::new().min_column_width(100.0);
        for &id in &ids {
            flow = flow.add_child(id);
        }
        tree.add(flow);
        tree.layout(SizeProposal::exact(200.0, 400.0));
        // 2 columns: [0,1] [2,3]
        assert!((tree.bounds(ids[2]).x - 100.0).abs() < 0.01);

        // Drop child 1: the live set is [0,2,3] -> [0,2] [3]
        tree.set_dormant(ids[1]);
        tree.layout(SizeProposal::exact(200.0, 400.0));
        assert!((tree.bounds(ids[0]).x - 0.0).abs() < 0.01);
        assert!(
            (tree.bounds(ids[2]).x - 0.0).abs() < 0.01,
            "child 2 -> col 0"
        );
        assert!((tree.bounds(ids[2]).y - 40.0).abs() < 0.01);
        assert!(
            (tree.bounds(ids[3]).x - 100.0).abs() < 0.01,
            "child 3 -> col 1"
        );
    }

    #[test]
    fn rtl_mirrors_columns_without_touching_source_order() {
        let mut tree = WidgetTree::new();
        tree.set_layout_direction(bastyde_core::environment::LayoutDirection::RightToLeft);
        let (ids, flow) = six_children(&mut tree);
        tree.layout(SizeProposal::exact(300.0, 400.0));

        // Logical column 0 sits at the trailing (right) edge under RTL.
        assert!((tree.bounds(ids[0]).x - 200.0).abs() < 0.01);
        assert!((tree.bounds(ids[2]).x - 100.0).abs() < 0.01);
        assert!((tree.bounds(ids[4]).x - 0.0).abs() < 0.01);
        // Mirroring is geometry only — children() order is untouched, so the
        // reading and focus order still follow the source.
        assert_eq!(tree.children(flow), ids);
    }

    #[test]
    fn column_count_signal_fires_only_on_a_real_change() {
        let mut tree = WidgetTree::new();
        let ids: Vec<_> = (0..6).map(|_| tree.add(FixedLeaf(50.0, 40.0))).collect();
        let flow = ColumnFlow::new().min_column_width(100.0);
        let count = flow.column_count_signal();
        let mut f = flow;
        for &id in &ids {
            f = f.add_child(id);
        }
        tree.add(f);

        let fires = std::rc::Rc::new(Cell::new(0usize));
        let seen = fires.clone();
        let _guard = count.observe(move |_| seen.set(seen.get() + 1));

        tree.layout(SizeProposal::exact(300.0, 400.0));
        assert_eq!(count.get(), 3);
        let after_first = fires.get();

        // Same width twice: no further notification.
        tree.layout(SizeProposal::exact(300.0, 400.0));
        assert_eq!(
            fires.get(),
            after_first,
            "re-layout at the same width is silent"
        );

        // Crossing to 2 columns fires exactly once.
        tree.layout(SizeProposal::exact(200.0, 400.0));
        assert_eq!(count.get(), 2);
        assert_eq!(fires.get(), after_first + 1);
    }

    // ── accessibility ────────────────────────────────────────────────

    fn find_node(
        update: &bastyde_core::accesskit::TreeUpdate,
        id: WidgetId,
    ) -> Option<&bastyde_core::accesskit::Node> {
        let nid = bastyde_core::accessibility::widget_id_to_node_id(id);
        update
            .nodes
            .iter()
            .find(|(n, _)| *n == nid)
            .map(|(_, node)| node)
    }

    fn nodes_with_role(
        update: &bastyde_core::accesskit::TreeUpdate,
        role: bastyde_core::accesskit::Role,
    ) -> Vec<&bastyde_core::accesskit::Node> {
        update
            .nodes
            .iter()
            .filter(|(_, n)| n.role() == role)
            .map(|(_, n)| n)
            .collect()
    }

    #[test]
    fn default_container_is_pruned_and_children_promoted_in_source_order() {
        let mut tree = WidgetTree::new();
        let labels = ["one", "two", "three", "four"];
        let ids: Vec<_> = labels
            .iter()
            .map(|&l| tree.add(LabeledLeaf(50.0, 40.0, l)))
            .collect();
        let mut flow = ColumnFlow::new().min_column_width(100.0);
        for &id in &ids {
            flow = flow.add_child(id);
        }
        let flow_id = tree.add(flow);
        tree.layout(SizeProposal::exact(200.0, 400.0));
        let update = tree.sync_accessibility();

        // A bare GenericContainer carries no semantics, so the walker drops it
        // and promotes the children — the correct result for a layout, which
        // contributes geometry rather than meaning.
        assert!(
            find_node(&update, flow_id).is_none(),
            "a property-free layout container must not reach assistive tech"
        );
        // The children survive; only the empty box went away.
        for &id in &ids {
            assert!(find_node(&update, id).is_some(), "child kept");
        }

        // And they are read in source order, not visual column order — the
        // keystone invariant. At 2 columns the visual layout is
        // [one, two] [three, four]; the reading order is still one..four.
        // ColumnFlow was the tree root, so its children promote all the way to
        // the synthetic Window root.
        let root = update
            .nodes
            .iter()
            .find(|(n, _)| *n == bastyde_core::accessibility::root_node_id())
            .map(|(_, node)| node)
            .expect("window root node");
        let order: Vec<_> = root
            .children()
            .iter()
            .filter_map(|nid| {
                update
                    .nodes
                    .iter()
                    .find(|(n, _)| n == nid)
                    .and_then(|(_, n)| n.label())
            })
            .collect();
        assert_eq!(order, labels, "promoted children keep source order");
    }

    #[test]
    fn semantic_list_emits_list_and_positioned_items() {
        let mut tree = WidgetTree::new();
        let ids: Vec<_> = (0..3).map(|_| tree.add(FixedLeaf(50.0, 40.0))).collect();
        let mut flow = ColumnFlow::new()
            .min_column_width(100.0)
            .semantic_list(true);
        for &id in &ids {
            flow = flow.add_child(id);
        }
        let flow_id = tree.add(flow);
        tree.layout(SizeProposal::exact(300.0, 400.0));
        let update = tree.sync_accessibility();

        let list = find_node(&update, flow_id).expect("List node survives pruning");
        assert_eq!(list.role(), bastyde_core::accesskit::Role::List);

        let items = nodes_with_role(&update, bastyde_core::accesskit::Role::ListItem);
        assert_eq!(items.len(), 3, "one ListItem per child");
        // Announced as "item N of 3", in source order.
        let mut seen: Vec<(usize, usize)> = items
            .iter()
            .map(|n| (n.position_in_set().unwrap(), n.size_of_set().unwrap()))
            .collect();
        seen.sort();
        assert_eq!(seen, vec![(1, 3), (2, 3), (3, 3)]);
    }

    // ── paint ────────────────────────────────────────────────────────

    /// Every x the column rule was drawn at, from a real render pass.
    /// `draw_line` lands in `decorations` or `cosmetic_lines` depending on the
    /// stroke space, so check both rather than assume.
    fn rule_xs(tree: &mut WidgetTree) -> Vec<f32> {
        let frame = tree.render();
        let mut xs: Vec<f32> = frame
            .cosmetic_lines
            .iter()
            .filter(|l| (l.from[0] - l.to[0]).abs() < 0.01) // vertical only
            .map(|l| l.from[0])
            .chain(
                frame
                    .decorations
                    .iter()
                    .filter(|d| d.rect[2] > 0.0 && d.rect[2] <= 2.0 && d.rect[3] > 10.0)
                    .map(|d| d.rect[0] + d.rect[2] / 2.0),
            )
            .collect();
        xs.sort_by(|a, b| a.partial_cmp(b).unwrap());
        xs
    }

    fn flow_with_rule(tree: &mut WidgetTree, rule: bool) -> WidgetId {
        let ids: Vec<_> = (0..6).map(|_| tree.add(FixedLeaf(50.0, 40.0))).collect();
        let mut flow = ColumnFlow::new().min_column_width(100.0);
        if rule {
            flow = flow.column_rule(1.0, bastyde_tokens::BorderRole::Divider);
        }
        for &id in &ids {
            flow = flow.add_child(id);
        }
        tree.add(flow)
    }

    #[test]
    fn column_rule_paints_one_line_centred_in_each_gap() {
        let mut tree = WidgetTree::new();
        flow_with_rule(&mut tree, true);
        // 3 columns of 100 in 300, no spacing -> gaps centred at 100 and 200.
        tree.layout(SizeProposal::exact(300.0, 400.0));
        let xs = rule_xs(&mut tree);
        assert_eq!(xs.len(), 2, "columns - 1 rules, got {xs:?}");
        assert!((xs[0] - 100.0).abs() < 0.01, "got {xs:?}");
        assert!((xs[1] - 200.0).abs() < 0.01, "got {xs:?}");
    }

    #[test]
    fn column_rule_follows_the_reflow() {
        let mut tree = WidgetTree::new();
        flow_with_rule(&mut tree, true);
        tree.layout(SizeProposal::exact(300.0, 400.0));
        assert_eq!(rule_xs(&mut tree).len(), 2, "3 columns -> 2 rules");

        tree.layout(SizeProposal::exact(200.0, 400.0));
        assert_eq!(rule_xs(&mut tree).len(), 1, "2 columns -> 1 rule");

        tree.layout(SizeProposal::exact(100.0, 400.0));
        assert!(
            rule_xs(&mut tree).is_empty(),
            "a single column has no gap to rule"
        );
    }

    #[test]
    fn no_rule_paints_nothing() {
        let mut tree = WidgetTree::new();
        flow_with_rule(&mut tree, false);
        tree.layout(SizeProposal::exact(300.0, 400.0));
        assert!(
            rule_xs(&mut tree).is_empty(),
            "column_rule is opt-in; the default layout paints nothing"
        );
    }

    #[test]
    fn column_rule_sits_in_the_gap_when_spacing_is_wide() {
        let mut tree = WidgetTree::new();
        let ids: Vec<_> = (0..4).map(|_| tree.add(FixedLeaf(50.0, 40.0))).collect();
        let mut flow = ColumnFlow::new()
            .min_column_width(100.0)
            .column_spacing(20.0)
            .column_rule(1.0, bastyde_tokens::BorderRole::Divider);
        for &id in &ids {
            flow = flow.add_child(id);
        }
        tree.add(flow);
        // floor((340 + 20) / 120) = 3 columns; width = (340 - 40)/3 = 100.
        // Gap 0 spans 100..120 -> rule at 110. Gap 1 spans 220..240 -> 230.
        tree.layout(SizeProposal::exact(340.0, 400.0));
        let xs = rule_xs(&mut tree);
        assert_eq!(xs.len(), 2, "got {xs:?}");
        assert!((xs[0] - 110.0).abs() < 0.01, "centred in gap 0, got {xs:?}");
        assert!((xs[1] - 230.0).abs() < 0.01, "centred in gap 1, got {xs:?}");
    }

    #[test]
    fn semantic_list_wrapper_is_layout_transparent() {
        // The wrapper must not perturb geometry.
        let mut tree = WidgetTree::new();
        let ids: Vec<_> = (0..4).map(|_| tree.add(FixedLeaf(50.0, 40.0))).collect();
        let mut flow = ColumnFlow::new()
            .min_column_width(100.0)
            .semantic_list(true);
        for &id in &ids {
            flow = flow.add_child(id);
        }
        tree.add(flow);
        tree.layout(SizeProposal::exact(200.0, 400.0));

        assert!((tree.bounds(ids[0]).x - 0.0).abs() < 0.01);
        assert!((tree.bounds(ids[0]).width - 100.0).abs() < 0.01);
        assert!((tree.bounds(ids[2]).x - 100.0).abs() < 0.01);
        assert!((tree.bounds(ids[1]).y - 40.0).abs() < 0.01);
    }
}

/// Property-based tests for [`balance_columns`].
///
/// `balance_columns` is `pub(crate)`, so this suite lives inline rather than
/// in `tests/` (an integration test cannot see it). The module docs above
/// state a handful of unusually crisp, checkable guarantees: children are
/// distributed as **contiguous source-order runs** (the property that keeps
/// visual order == focus order == the a11y walk order — see the "Reading
/// order" section at the top of this file), the partition uses **exactly**
/// `k` columns whenever `n >= k` with **no column left empty**, the result is
/// **deterministic** across repeated calls (`layout_response` and
/// `place_children` each re-run the search from scratch with no persisted
/// state, so a disagreement between two calls would desynchronise measurement
/// from placement), and the balanced tallest column is never worse than a
/// naive same-count-per-column split (the oracle this bisection search
/// replaces).
///
/// `cargo-fuzz` needs nightly + libfuzzer-sys, which isn't assumed here;
/// proptest with 256–512 cases per property (override with
/// `PROPTEST_CASES=N`) gives the "never panics / never regresses on a weird
/// shape" coverage a fuzz corpus would, plus shrinking. See `mod tests` above
/// for the example-based regression coverage this suite deliberately does not
/// repeat (the empty-trailing-column bug, the zero-height-still-pays-the-gap
/// bug, etc.).
#[cfg(test)]
mod proptests {
    use super::*;
    use proptest::prelude::*;

    // Zero and small heights are the specific edge case `columns_needed`
    // guards against (a run of zero-height items is still `n` items with
    // `n-1` gaps between them) — bias toward hitting them.
    fn arb_height() -> impl Strategy<Value = f32> {
        prop_oneof![Just(0.0_f32), 0.0f32..500.0_f32,]
    }

    fn arb_heights() -> impl Strategy<Value = Vec<f32>> {
        prop::collection::vec(arb_height(), 0..24)
    }

    // Zero, negative (clamped), and huge gaps are the documented edge cases;
    // a mid-range gap is the common case.
    fn arb_gap() -> impl Strategy<Value = f32> {
        prop_oneof![
            Just(0.0_f32),
            Just(-5.0_f32),
            0.0f32..50.0_f32,
            Just(10_000.0_f32),
        ]
    }

    // A non-negative gap for properties that compare against an oracle
    // computed with the same, unclamped gap value.
    fn arb_nonneg_gap() -> impl Strategy<Value = f32> {
        prop_oneof![Just(0.0_f32), 0.0f32..50.0_f32, Just(5_000.0_f32),]
    }

    // k == 0 and k > n are the documented degenerate cases; small k is the
    // common case.
    fn arb_k() -> impl Strategy<Value = usize> {
        prop_oneof![Just(0usize), 1usize..8usize,]
    }

    /// Same-count-per-column split: assign `heights` to `k` columns as
    /// contiguous runs of near-equal *count*, ignoring the heights entirely.
    /// This is the textbook naive multi-column partition.
    ///
    /// Why this (rather than literally re-implementing the "greedy fill"
    /// mentioned in the module docs) is a sound comparison oracle:
    /// `columns_needed` is a monotone feasibility check (a higher limit
    /// never needs more columns), so bisecting it finds the smallest limit
    /// any contiguous partition into `k_eff` runs can achieve — i.e. the
    /// *true minimum* possible tallest-column extent over **every** valid
    /// `k_eff`-way contiguous partition, not just ones `balance_columns`
    /// happens to construct. (The reserve tweak in `balance_columns` only
    /// ever makes an earlier column take *fewer* items to keep every column
    /// non-empty — it can't push a column's extent past the bisected limit,
    /// since splitting a feasible run into two contiguous sub-runs can only
    /// keep or shrink each half's extent.) Given that, `balance_columns`'
    /// tallest column is, by construction, less than or equal to *any*
    /// specific `k`-way contiguous partition — the even-count split above,
    /// a hand-rolled greedy-fill-at-the-average, or anything else — so this
    /// oracle is valid regardless of which "naive" strategy is picked; the
    /// even-count split is simply the simplest one to implement correctly.
    fn naive_even_split_extents(heights: &[f32], gap: f32, k: usize) -> Vec<f32> {
        let n = heights.len();
        if n == 0 {
            return Vec::new();
        }
        let k_eff = k.min(n).max(1);
        let base = n / k_eff;
        let extra = n % k_eff;
        let mut extents = Vec::with_capacity(k_eff);
        let mut idx = 0usize;
        for col in 0..k_eff {
            let take = base + usize::from(col < extra);
            let slice = &heights[idx..idx + take];
            let sum: f32 = slice.iter().sum();
            extents.push(run_extent(sum, take, gap));
            idx += take;
        }
        extents
    }

    // ── 1. partition is a set of contiguous, source-order runs ──
    proptest! {
        #[test]
        fn column_indices_never_decrease_across_the_source_order(
            heights in arb_heights(), gap in arb_gap(), k in arb_k(),
        ) {
            // column_of is non-decreasing in i, which is exactly what makes
            // each column's original indices a contiguous block, and
            // concatenating the columns in order reproduces 0..n exactly —
            // the property that keeps visual order == focus order == the
            // a11y walk order (see the "Reading order" module docs above).
            let r = balance_columns(&heights, gap, k);
            for w in r.column_of.windows(2) {
                prop_assert!(
                    w[1] >= w[0],
                    "column index went backwards in {:?}", r.column_of
                );
            }
        }
    }

    // ── 2. exactly k columns are used whenever n >= k ──
    proptest! {
        #[test]
        fn uses_exactly_k_columns_when_there_are_enough_items(
            heights in arb_heights(), gap in arb_gap(), k in 1usize..8usize,
        ) {
            let n = heights.len();
            prop_assume!(n >= k);
            let r = balance_columns(&heights, gap, k);
            let used = r.column_of.iter().copied().max().map_or(0, |m| m + 1);
            prop_assert_eq!(
                used, k,
                "expected exactly {} columns for {} items, used {}", k, n, used
            );
        }
    }

    // ── 3. no column is left empty when n >= k ──
    proptest! {
        #[test]
        fn no_column_is_empty_when_there_are_enough_items(
            heights in arb_heights(), gap in arb_gap(), k in 1usize..8usize,
        ) {
            let n = heights.len();
            prop_assume!(n >= k);
            let r = balance_columns(&heights, gap, k);
            for col in 0..k {
                prop_assert!(
                    r.column_of.contains(&col),
                    "column {} is empty in partition {:?}", col, r.column_of
                );
            }
        }
    }

    // ── 4. balance never does worse than a naive even-count split ──
    proptest! {
        #![proptest_config(ProptestConfig { cases: 512, ..ProptestConfig::default() })]
        #[test]
        fn tallest_column_is_at_most_the_naive_even_split(
            heights in arb_heights(), gap in arb_nonneg_gap(), k in arb_k(),
        ) {
            let r = balance_columns(&heights, gap, k);
            let naive_tallest = naive_even_split_extents(&heights, gap, k)
                .into_iter()
                .fold(0.0_f32, f32::max);
            prop_assert!(
                r.height <= naive_tallest + 0.01,
                "balanced height {} exceeds naive even-split height {} for {:?} gap {} k {}",
                r.height, naive_tallest, heights, gap, k
            );
        }
    }

    // ── 5. determinism across repeated calls ──
    proptest! {
        #[test]
        fn repeated_calls_on_the_same_input_agree_bit_for_bit(
            heights in arb_heights(), gap in arb_gap(), k in arb_k(),
        ) {
            // layout_response and place_children each run the bisection from
            // scratch; a disagreement here would desynchronise measured size
            // from placed geometry.
            let a = balance_columns(&heights, gap, k);
            let b = balance_columns(&heights, gap, k);
            prop_assert_eq!(
                &a, &b,
                "two calls with identical input ({:?}, gap {}, k {}) produced different partitions: {:?} vs {:?}",
                heights, gap, k, a, b
            );
        }
    }

    // ── 6. reported height matches the reconstructed tallest column ──
    proptest! {
        #[test]
        fn reported_height_matches_the_reconstructed_tallest_column(
            heights in arb_heights(), gap in arb_gap(), k in arb_k(),
        ) {
            let r = balance_columns(&heights, gap, k);
            let cols = r.column_of.iter().copied().max().map_or(0, |m| m + 1);
            let mut sums = vec![0.0f32; cols];
            let mut counts = vec![0usize; cols];
            for (i, &h) in heights.iter().enumerate() {
                counts[r.column_of[i]] += 1;
                sums[r.column_of[i]] += h;
            }
            let clamped_gap = gap.max(0.0);
            let tallest = (0..cols)
                .map(|c| run_extent(sums[c], counts[c], clamped_gap))
                .fold(0.0_f32, f32::max);
            prop_assert!(
                (r.height - tallest).abs() < 0.05,
                "reported height {} disagrees with reconstructed tallest column {}",
                r.height, tallest
            );
        }
    }

    // ── 7. no column ever exceeds the reported height ──
    proptest! {
        #[test]
        fn no_column_extent_exceeds_the_reported_height(
            heights in arb_heights(), gap in arb_gap(), k in arb_k(),
        ) {
            let r = balance_columns(&heights, gap, k);
            let cols = r.column_of.iter().copied().max().map_or(0, |m| m + 1);
            let mut sums = vec![0.0f32; cols];
            let mut counts = vec![0usize; cols];
            for (i, &h) in heights.iter().enumerate() {
                counts[r.column_of[i]] += 1;
                sums[r.column_of[i]] += h;
            }
            let clamped_gap = gap.max(0.0);
            for c in 0..cols {
                let extent = run_extent(sums[c], counts[c], clamped_gap);
                prop_assert!(
                    extent <= r.height + 0.05,
                    "column {} extent {} exceeds reported height {}", c, extent, r.height
                );
            }
        }
    }

    // ── 8. never panics on degenerate shapes (n == 0, k == 0, k > n, huge gap) ──
    proptest! {
        #[test]
        fn never_panics_on_degenerate_input(
            heights in prop::collection::vec(arb_height(), 0..3),
            gap in prop_oneof![Just(0.0_f32), Just(-1.0_f32), Just(1.0e6_f32)],
            k in prop_oneof![Just(0usize), Just(1usize), Just(100usize)],
        ) {
            let n = heights.len();
            let r = balance_columns(&heights, gap, k);
            prop_assert_eq!(
                r.column_of.len(), n,
                "every child must be assigned a column: heights {:?} gap {} k {} -> {:?}",
                heights, gap, k, r.column_of
            );
            // Every assigned column index must be a valid index into the
            // partition (`k_eff = k.min(n).max(1) <= n` whenever n >= 1), even
            // when k wildly overshoots n (k = 100 against at most 2 items).
            prop_assert!(
                r.column_of.iter().all(|&c| c < n.max(1)),
                "out-of-range column index in {:?} for {} items (gap {} k {})",
                r.column_of, n, gap, k
            );
            prop_assert!(
                r.height.is_finite() && r.height >= 0.0,
                "height {} is not a finite, non-negative number for heights {:?} gap {} k {}",
                r.height, heights, gap, k
            );
        }
    }
}
