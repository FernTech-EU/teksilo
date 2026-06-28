// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! MasonryLayout — a variable-height grid that packs children into the
//! shortest column (Pinterest-style).
//!
//! Each child is measured at the shared column width and placed into
//! whichever column currently has the lowest accumulated height.
//! Ties between equal-height columns are broken by column index
//! (leftmost wins). All columns share the same width; column and item
//! spacing are independently configurable. RTL layout mirrors the
//! column order so the first logical child still goes to the leading edge.
//!
//! ```rust
//! # use bastyde_widgets::primitives::masonry::MasonryLayout;
//! # use bastyde_widgets::primitives::TextWidget;
//! # use bastyde_i18n::lit;
//! let _grid = MasonryLayout::new(3)
//!     .column_spacing(8.0)
//!     .item_spacing(8.0)
//!     .child(TextWidget::new(lit!("Tall card")))
//!     .child(TextWidget::new(lit!("Short card")))
//!     .child(TextWidget::new(lit!("Another card")));
//! ```

use bastyde_canvas::{Point, Rect, Size, SizeProposal};
use bastyde_core::accessibility::AccessNodeBuilder;
use bastyde_core::widget::{LayoutContext, PaintContext, PendingChild, Widget, WidgetPlacement};
use bastyde_core::widget_id::WidgetId;

/// A masonry (Pinterest-style) layout that packs children into the shortest
/// column.
///
/// Children are placed left-to-right into whichever column is currently
/// shortest. All children receive the same column width; their heights are
/// determined by each child's intrinsic size at that width.
///
/// ```text
/// ┌──────┐ ┌──────┐ ┌──────┐
/// │  A   │ │  B   │ │  C   │
/// │      │ │      │ └──────┘
/// │      │ └──────┘ ┌──────┐
/// └──────┘ ┌──────┐ │  F   │
/// ┌──────┐ │  E   │ │      │
/// │  D   │ └──────┘ └──────┘
/// └──────┘
/// ```
#[derive(Debug)]
pub struct MasonryLayout {
    column_count: usize,
    column_spacing: f32,
    item_spacing: f32,
    child_ids: Vec<WidgetId>,
    pending: Vec<PendingChild>,
}

impl MasonryLayout {
    /// Create a masonry layout with the given number of columns.
    ///
    /// The count is clamped to a minimum of 1.
    pub fn new(column_count: usize) -> Self {
        Self {
            column_count: column_count.max(1),
            column_spacing: 0.0,
            item_spacing: 0.0,
            child_ids: Vec::new(),
            pending: Vec::new(),
        }
    }

    /// Horizontal gap between columns.
    pub fn column_spacing(mut self, spacing: f32) -> Self {
        self.column_spacing = spacing;
        self
    }

    /// Vertical gap between items within the same column.
    pub fn item_spacing(mut self, spacing: f32) -> Self {
        self.item_spacing = spacing;
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

    /// Width of each column given the total available width.
    fn column_width(&self, available_width: f32) -> f32 {
        let gaps = self.column_spacing * (self.column_count as f32 - 1.0).max(0.0);
        ((available_width - gaps) / self.column_count as f32).max(0.0)
    }

    /// Index of the shortest column (lowest accumulated height).
    /// Ties are broken by lowest index (leftmost column first).
    fn shortest_column(col_heights: &[f32]) -> usize {
        col_heights
            .iter()
            .enumerate()
            .min_by(|(_, a), (_, b)| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal))
            .map(|(i, _)| i)
            .unwrap_or(0)
    }
}

impl Default for MasonryLayout {
    fn default() -> Self {
        Self::new(2)
    }
}

impl Widget for MasonryLayout {
    fn build(&mut self, ctx: &mut bastyde_core::build_context::BuildContext) -> Vec<WidgetId> {
        let pending = std::mem::take(&mut self.pending);
        if !pending.is_empty() {
            self.child_ids = pending
                .into_iter()
                .map(|child| match child {
                    PendingChild::Id(id) => id,
                    PendingChild::Deferred(w) => ctx.add_boxed(w),
                })
                .collect();
        }
        self.child_ids.clone()
    }

    fn layout_response(
        &self,
        proposal: SizeProposal,
        ctx: &LayoutContext,
    ) -> bastyde_core::widget::LayoutResponse {
        if self.child_ids.is_empty() {
            return (proposal.resolve(0.0, 0.0)).into();
        }

        let (total_width, col_width) = if let Some(w) = proposal.width {
            (w, self.column_width(w))
        } else {
            // Unbounded: use the widest child's intrinsic width as column width.
            let mut max_w = 0.0_f32;
            for &child_id in &self.child_ids {
                if let Some(s) = ctx.child_size(child_id, SizeProposal::unspecified()) {
                    max_w = max_w.max(s.width);
                }
            }
            let gaps = self.column_spacing * (self.column_count as f32 - 1.0).max(0.0);
            let total = max_w * self.column_count as f32 + gaps;
            (total, max_w)
        };

        // Measure each child at column width, simulate placement.
        let child_proposal = SizeProposal::with_width(col_width);
        let mut col_heights = vec![0.0_f32; self.column_count];

        for &child_id in &self.child_ids {
            if let Some(child_size) = ctx.child_size(child_id, child_proposal) {
                let col = Self::shortest_column(&col_heights);
                if col_heights[col] > 0.0 {
                    col_heights[col] += self.item_spacing;
                }
                col_heights[col] += child_size.height;
            }
        }

        let total_height = col_heights.iter().copied().fold(0.0_f32, f32::max);
        Size::new(total_width, total_height).into()
    }

    fn place_children(
        &self,
        bounds: Rect,
        _proposal: SizeProposal,
        children: &mut [WidgetPlacement],
        ctx: &LayoutContext,
    ) {
        if children.is_empty() {
            return;
        }

        let col_width = self.column_width(bounds.width);
        let rtl = ctx.is_rtl();

        // Column X origins (mirrored for RTL).
        let col_x: Vec<f32> = (0..self.column_count)
            .map(|i| {
                let physical_col = if rtl { self.column_count - 1 - i } else { i };
                bounds.x + physical_col as f32 * (col_width + self.column_spacing)
            })
            .collect();

        let mut col_y = vec![bounds.y; self.column_count];

        let child_proposal = SizeProposal::with_width(col_width);
        for child in children.iter_mut() {
            let child_size = ctx
                .child_size(child.id, child_proposal)
                .unwrap_or(Size::ZERO);

            let col = Self::shortest_column(&col_y);

            if col_y[col] > bounds.y {
                col_y[col] += self.item_spacing;
            }

            child.origin = Point::new(col_x[col], col_y[col]);
            child.size = Size::new(col_width, child_size.height);
            col_y[col] += child_size.height;
        }
    }

    fn paint(&self, _bounds: Rect, _canvas: &mut bastyde_canvas::Canvas, _ctx: &PaintContext) {}

    fn accessibility(&self, builder: &mut AccessNodeBuilder) {
        builder.set_role(bastyde_core::accesskit::Role::GenericContainer);
    }

    fn children(&self) -> Vec<WidgetId> {
        self.child_ids.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bastyde_core::widget_tree::WidgetTree;

    #[derive(Debug)]
    struct FixedLeaf(f32, f32);
    impl Widget for FixedLeaf {
        fn layout_response(
            &self,
            _proposal: SizeProposal,
            _ctx: &LayoutContext,
        ) -> bastyde_core::widget::LayoutResponse {
            Size::new(self.0, self.1).into()
        }
    }

    #[test]
    fn equal_height_items_fill_columns_evenly() {
        let mut tree = WidgetTree::new();
        let items: Vec<_> = (0..6).map(|_| tree.add(FixedLeaf(50.0, 40.0))).collect();
        let _m = tree.add(
            MasonryLayout::new(3)
                .add_child(items[0])
                .add_child(items[1])
                .add_child(items[2])
                .add_child(items[3])
                .add_child(items[4])
                .add_child(items[5]),
        );
        // 3 columns in 300px: col_width = 100.0
        tree.layout(SizeProposal::exact(300.0, 400.0));

        // First row: items 0,1,2 → cols 0,1,2 at y=0
        assert!((tree.bounds(items[0]).y - 0.0).abs() < 0.01);
        assert!((tree.bounds(items[1]).y - 0.0).abs() < 0.01);
        assert!((tree.bounds(items[2]).y - 0.0).abs() < 0.01);
        // Second row: items 3,4,5 → cols 0,1,2 at y=40
        assert!((tree.bounds(items[3]).y - 40.0).abs() < 0.01);
        assert!((tree.bounds(items[4]).y - 40.0).abs() < 0.01);
        assert!((tree.bounds(items[5]).y - 40.0).abs() < 0.01);
    }

    #[test]
    fn variable_height_items_go_to_shortest_column() {
        let mut tree = WidgetTree::new();
        // Item 0 is tall, items 1-3 are short.
        let a = tree.add(FixedLeaf(50.0, 100.0));
        let b = tree.add(FixedLeaf(50.0, 30.0));
        let c = tree.add(FixedLeaf(50.0, 30.0));
        let d = tree.add(FixedLeaf(50.0, 20.0));
        let _m = tree.add(
            MasonryLayout::new(3)
                .add_child(a)
                .add_child(b)
                .add_child(c)
                .add_child(d),
        );
        tree.layout(SizeProposal::exact(300.0, 400.0));

        // a → col 0 (all at 0), b → col 1, c → col 2
        // Heights: [100, 30, 30]. d → col 1 (tied at 30, lowest index wins).
        assert!((tree.bounds(d).x - 100.0).abs() < 0.01); // col 1 starts at 100
        assert!((tree.bounds(d).y - 30.0).abs() < 0.01); // below b
    }

    #[test]
    fn column_spacing_applied() {
        let mut tree = WidgetTree::new();
        let a = tree.add(FixedLeaf(50.0, 40.0));
        let b = tree.add(FixedLeaf(50.0, 40.0));
        let c = tree.add(FixedLeaf(50.0, 40.0));
        let _m = tree.add(
            MasonryLayout::new(3)
                .column_spacing(10.0)
                .add_child(a)
                .add_child(b)
                .add_child(c),
        );
        // 3 cols, spacing 10: col_width = (320 - 2*10) / 3 = 100
        tree.layout(SizeProposal::exact(320.0, 200.0));

        assert!((tree.bounds(a).x - 0.0).abs() < 0.01);
        assert!((tree.bounds(b).x - 110.0).abs() < 0.01); // 100 + 10
        assert!((tree.bounds(c).x - 220.0).abs() < 0.01); // 200 + 20
    }

    #[test]
    fn item_spacing_applied() {
        let mut tree = WidgetTree::new();
        let a = tree.add(FixedLeaf(50.0, 40.0));
        let b = tree.add(FixedLeaf(50.0, 50.0));
        let c = tree.add(FixedLeaf(50.0, 20.0));
        let d = tree.add(FixedLeaf(50.0, 20.0));
        let _m = tree.add(
            MasonryLayout::new(2)
                .item_spacing(8.0)
                .add_child(a)
                .add_child(b)
                .add_child(c)
                .add_child(d),
        );
        tree.layout(SizeProposal::exact(200.0, 400.0));

        // a → col 0 at y=0, b → col 1 at y=0
        // Heights: [40, 50]. c → col 0 (shorter), y = 40 + 8 = 48
        assert!((tree.bounds(c).y - 48.0).abs() < 0.01);
        // Heights: [40+8+20=68, 50]. d → col 1 (shorter), y = 50 + 8 = 58
        assert!((tree.bounds(d).y - 58.0).abs() < 0.01);
    }

    #[test]
    fn intrinsic_height_is_tallest_column() {
        let mut tree = WidgetTree::new();
        let a = tree.add(FixedLeaf(50.0, 100.0));
        let b = tree.add(FixedLeaf(50.0, 30.0));
        let c = tree.add(FixedLeaf(50.0, 30.0));
        let m = tree.add(MasonryLayout::new(2).add_child(a).add_child(b).add_child(c));
        tree.layout(SizeProposal {
            width: Some(200.0),
            height: None,
        });

        // a → col 0 (height 100), b → col 1 (height 30), c → col 1 (height 60)
        // Tallest column = col 0 at 100.
        assert!((tree.bounds(m).height - 100.0).abs() < 0.01);
    }

    #[test]
    fn single_child_goes_to_first_column() {
        let mut tree = WidgetTree::new();
        let a = tree.add(FixedLeaf(50.0, 40.0));
        let _m = tree.add(MasonryLayout::new(3).add_child(a));
        tree.layout(SizeProposal::exact(300.0, 200.0));

        assert!((tree.bounds(a).x - 0.0).abs() < 0.01);
        assert!((tree.bounds(a).y - 0.0).abs() < 0.01);
    }

    #[test]
    fn empty_masonry_has_zero_size() {
        let mut tree = WidgetTree::new();
        let m = tree.add(MasonryLayout::new(3));
        tree.layout(SizeProposal {
            width: Some(300.0),
            height: None,
        });

        assert!((tree.bounds(m).height - 0.0).abs() < 0.01);
    }

    #[test]
    fn dormant_child_excluded_from_layout() {
        let mut tree = WidgetTree::new();
        let a = tree.add(FixedLeaf(50.0, 40.0));
        let b = tree.add(FixedLeaf(50.0, 30.0));
        let c = tree.add(FixedLeaf(50.0, 20.0));
        let _m = tree.add(MasonryLayout::new(2).add_child(a).add_child(b).add_child(c));
        tree.layout(SizeProposal::exact(200.0, 200.0));

        // a → col 0, b → col 1, c → col 1 (shorter at 30 vs 40)
        assert!((tree.bounds(c).x - 100.0).abs() < 0.01); // col 1

        // Make b dormant: remaining are a and c
        tree.set_dormant(b);
        tree.layout(SizeProposal::exact(200.0, 200.0));

        // a → col 0, c → col 1 (both start at 0)
        assert!((tree.bounds(a).x - 0.0).abs() < 0.01);
        assert!((tree.bounds(c).x - 100.0).abs() < 0.01);
        assert!((tree.bounds(c).y - 0.0).abs() < 0.01);
    }

    #[test]
    fn children_receive_column_width() {
        let mut tree = WidgetTree::new();
        let a = tree.add(FixedLeaf(50.0, 40.0));
        let b = tree.add(FixedLeaf(50.0, 40.0));
        let _m = tree.add(MasonryLayout::new(2).add_child(a).add_child(b));
        // 2 cols in 200px: col_width = 100
        tree.layout(SizeProposal::exact(200.0, 200.0));

        // Placed width should be col_width (100), not intrinsic (50).
        assert!((tree.bounds(a).width - 100.0).abs() < 0.01);
        assert!((tree.bounds(b).width - 100.0).abs() < 0.01);
    }

    #[test]
    fn fewer_children_than_columns() {
        let mut tree = WidgetTree::new();
        let a = tree.add(FixedLeaf(50.0, 40.0));
        let b = tree.add(FixedLeaf(50.0, 30.0));
        let _m = tree.add(MasonryLayout::new(4).add_child(a).add_child(b));
        tree.layout(SizeProposal::exact(400.0, 200.0));

        // 4 cols, col_width = 100. a → col 0, b → col 1
        assert!((tree.bounds(a).x - 0.0).abs() < 0.01);
        assert!((tree.bounds(b).x - 100.0).abs() < 0.01);
    }

    #[test]
    fn rtl_mirrors_column_order() {
        let mut tree = WidgetTree::new();
        tree.set_layout_direction(bastyde_core::environment::LayoutDirection::RightToLeft);
        let a = tree.add(FixedLeaf(50.0, 40.0));
        let b = tree.add(FixedLeaf(50.0, 30.0));
        let c = tree.add(FixedLeaf(50.0, 20.0));
        let _m = tree.add(MasonryLayout::new(3).add_child(a).add_child(b).add_child(c));
        // 3 cols in 300px: col_width = 100
        tree.layout(SizeProposal::exact(300.0, 200.0));

        // In RTL, logical col 0 maps to rightmost physical position.
        // a → logical col 0 → physical x = 200
        // b → logical col 1 → physical x = 100
        // c → logical col 2 → physical x = 0
        assert!((tree.bounds(a).x - 200.0).abs() < 0.01);
        assert!((tree.bounds(b).x - 100.0).abs() < 0.01);
        assert!((tree.bounds(c).x - 0.0).abs() < 0.01);
    }

    #[test]
    fn unbounded_width_uses_intrinsic() {
        let mut tree = WidgetTree::new();
        let a = tree.add(FixedLeaf(80.0, 40.0));
        let b = tree.add(FixedLeaf(60.0, 30.0));
        let m = tree.add(MasonryLayout::new(3).add_child(a).add_child(b));
        tree.layout(SizeProposal {
            width: None,
            height: Some(200.0),
        });

        // Max intrinsic width = 80. Total = 3 * 80 + 0 gaps = 240.
        assert!((tree.bounds(m).width - 240.0).abs() < 0.01);
    }
}
