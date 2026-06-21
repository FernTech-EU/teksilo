// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

use bastyde_canvas::{Canvas, Point, Rect, Size, SizeProposal};

use bastyde_core::WidgetId;
use bastyde_core::accessibility::AccessNodeBuilder;
use bastyde_core::widget::{LayoutContext, PaintContext, PendingChild, Widget, WidgetPlacement};
use bastyde_tokens::Alignment;

/// A layout container that stacks children on top of each other.
/// Size = max of children sizes. Children are positioned according to
/// the container's `Alignment` (default: center), with per-child overrides.
#[derive(Debug)]
pub struct ZStack {
    child_ids: Vec<WidgetId>,
    pending: Vec<PendingChild>,
    alignment: Alignment,
}

impl ZStack {
    pub fn new() -> Self {
        Self {
            child_ids: Vec::new(),
            pending: Vec::new(),
            alignment: Alignment::CENTER,
        }
    }

    pub fn alignment(mut self, alignment: Alignment) -> Self {
        self.alignment = alignment;
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

    /// Conditionally add a child. No-op if None.
    pub fn child_opt(mut self, widget: Option<impl Widget + 'static>) -> Self {
        if let Some(w) = widget {
            self.pending.push(PendingChild::Deferred(Box::new(w)));
        }
        self
    }
}

impl Default for ZStack {
    fn default() -> Self {
        Self::new()
    }
}

impl Widget for ZStack {
    fn layout_response(
        &self,
        proposal: SizeProposal,
        ctx: &LayoutContext,
    ) -> bastyde_core::widget::LayoutResponse {
        // Ask each child for its intrinsic size (unspecified proposal) and take the max.
        // This ensures background elements (like RectWidget, which returns 0x0 for
        // unspecified) don't inflate the stack's size.
        //
        // NOTE: a known limitation is that a height-for-width child (wrapping
        // text) measured here at `unspecified()` reports its single-line width
        // rather than wrapping to the offered width. Forwarding the incoming
        // `proposal` instead would fix wrapping, but it interacts badly with
        // `MinSize` (which forwards `Some(min)` as the width): a *shrinkable*
        // single-line label inside `MinSize → ZStack` then truncates to the min
        // width during intrinsic measurement. A correct fix needs `MinSize`'s
        // proposal semantics reworked for shrink- vs. wrap-aware children, so
        // the conservative `unspecified()` is kept here for now.
        let mut max_w: f32 = 0.0;
        let mut max_h: f32 = 0.0;
        let mut min_w: f32 = 0.0;
        let mut min_h: f32 = 0.0;
        let mut any_shrink = false;
        let mut any_queried = false;
        for &child_id in &self.child_ids {
            if let Some(r) = ctx.child_layout_response(child_id, SizeProposal::unspecified()) {
                max_w = max_w.max(r.size.width);
                max_h = max_h.max(r.size.height);
                min_w = min_w.max(r.min.width);
                min_h = min_h.max(r.min.height);
                if r.shrink > 0.0 {
                    any_shrink = true;
                }
                any_queried = true;
            }
        }
        if any_queried {
            // Size = max of children. Propagate a shrink weight + compression
            // floor when any child can shrink (so a ZStack wrapping shrinkable
            // content stays shrinkable), but keep `flex = 0`: a ZStack does not
            // claim growth slack.
            let size = Size::new(max_w, max_h);
            let min = Size::new(min_w.min(max_w), min_h.min(max_h));
            bastyde_core::widget::LayoutResponse::rigid(size)
                .with_shrink(if any_shrink { 1.0 } else { 0.0 })
                .with_min(min)
        } else {
            proposal.resolve(0.0, 0.0).into()
        }
    }

    fn place_children(
        &self,
        bounds: Rect,
        _proposal: SizeProposal,
        children: &mut [WidgetPlacement],
        ctx: &LayoutContext,
    ) {
        let rtl = ctx.is_rtl();
        let exact_proposal = SizeProposal::exact(bounds.width, bounds.height);
        for child in children.iter_mut() {
            // Query child with the full bounds as proposal. Children that accept
            // the proposal (e.g. background rects) fill the ZStack. Children with
            // fixed intrinsic size get their natural size and are positioned by alignment.
            let child_size = ctx
                .child_size(child.id, exact_proposal)
                .unwrap_or(bounds.size());

            // Per-child override or container default
            let align = ctx.child_alignment(child.id).unwrap_or(self.alignment);
            let (dx, dy) = align.resolve(
                (child_size.width, child_size.height),
                (bounds.width, bounds.height),
                rtl,
            );

            child.origin = Point::new(bounds.x + dx, bounds.y + dy);
            child.size = child_size;
        }
    }

    fn paint(&self, _bounds: Rect, _canvas: &mut Canvas, _ctx: &PaintContext) {}

    fn accessibility(&self, builder: &mut AccessNodeBuilder) {
        builder.set_role(bastyde_core::accesskit::Role::GenericContainer);
    }

    fn children(&self) -> Vec<WidgetId> {
        self.child_ids.clone()
    }

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
    fn default_centers_children() {
        let mut tree = WidgetTree::new();
        let a = tree.add(FixedLeaf(40.0, 20.0));
        let _stack = tree.add(ZStack::new().add_child(a));
        tree.layout(SizeProposal::exact(100.0, 60.0));

        // Root widget gets the full 100x60 proposal bounds.
        // Child 40x20 centered in 100x60: x=(100-40)/2=30, y=(60-20)/2=20
        let b = tree.bounds(a);
        assert!((b.x - 30.0).abs() < 0.01); // centered in 100: (100-40)/2=30
        assert!((b.y - 20.0).abs() < 0.01); // centered in 60: (60-20)/2=20
    }

    #[test]
    fn alignment_top_leading() {
        let mut tree = WidgetTree::new();
        let bg = tree.add(FixedLeaf(100.0, 60.0)); // large child sets ZStack size
        let fg = tree.add(FixedLeaf(40.0, 20.0));
        let _stack = tree.add(
            ZStack::new()
                .alignment(Alignment::TOP_LEADING)
                .add_child(bg)
                .add_child(fg),
        );
        tree.layout(SizeProposal::exact(200.0, 200.0));

        let b = tree.bounds(fg);
        assert!((b.x - 0.0).abs() < 0.01);
        assert!((b.y - 0.0).abs() < 0.01);
    }

    #[test]
    fn alignment_bottom_trailing() {
        let mut tree = WidgetTree::new();
        let bg = tree.add(FixedLeaf(100.0, 60.0));
        let fg = tree.add(FixedLeaf(40.0, 20.0));
        let _stack = tree.add(
            ZStack::new()
                .alignment(Alignment::BOTTOM_TRAILING)
                .add_child(bg)
                .add_child(fg),
        );
        tree.layout(SizeProposal::exact(200.0, 200.0));

        let b = tree.bounds(fg);
        assert!((b.x - 160.0).abs() < 0.01); // 200-40
        assert!((b.y - 180.0).abs() < 0.01); // 200-20
    }

    #[test]
    fn alignment_center() {
        let mut tree = WidgetTree::new();
        let bg = tree.add(FixedLeaf(100.0, 60.0));
        let fg = tree.add(FixedLeaf(40.0, 20.0));
        let _stack = tree.add(ZStack::new().add_child(bg).add_child(fg)); // default: center
        tree.layout(SizeProposal::exact(200.0, 200.0));

        let b = tree.bounds(fg);
        assert!((b.x - 80.0).abs() < 0.01); // (200-40)/2
        assert!((b.y - 90.0).abs() < 0.01); // (200-20)/2
    }

    #[test]
    fn per_child_alignment_override() {
        let mut tree = WidgetTree::new();
        let bg = tree.add(FixedLeaf(100.0, 60.0));
        let a = tree.add(FixedLeaf(40.0, 20.0));
        let b = tree.add(FixedLeaf(30.0, 15.0));
        let _stack = tree.add(
            ZStack::new()
                .alignment(Alignment::TOP_LEADING)
                .add_child(bg)
                .add_child(a)
                .add_child(b),
        );
        // Override b to bottom-trailing
        tree.set_alignment(b, Alignment::BOTTOM_TRAILING);
        tree.layout(SizeProposal::exact(200.0, 200.0));

        let ab = tree.bounds(a);
        assert!((ab.x - 0.0).abs() < 0.01); // top-leading
        assert!((ab.y - 0.0).abs() < 0.01);

        let bb = tree.bounds(b);
        assert!((bb.x - 170.0).abs() < 0.01); // 200-30
        assert!((bb.y - 185.0).abs() < 0.01); // 200-15
    }

    #[test]
    fn size_is_max_of_children() {
        let mut tree = WidgetTree::new();
        let a = tree.add(FixedLeaf(40.0, 60.0));
        let b = tree.add(FixedLeaf(80.0, 30.0));
        let stack = tree.add(ZStack::new().add_child(a).add_child(b));
        tree.layout(SizeProposal {
            width: None,
            height: None,
        });

        let sb = tree.bounds(stack);
        assert!((sb.width - 80.0).abs() < 0.01);
        assert!((sb.height - 60.0).abs() < 0.01);
    }
}
