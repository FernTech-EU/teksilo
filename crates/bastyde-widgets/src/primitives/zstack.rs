// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! ZStack — a layout container that layers children on top of each other.
//!
//! The container sizes itself to the maximum width and maximum height across
//! all children, measured at an unspecified proposal so background rects do not
//! inflate the size. **Height additionally takes a width-bounded query** when the
//! parent bound the width, so a wrapping child reports the height it will really
//! occupy rather than a single line; see `layout_response` for why that query is
//! width-only. Each child is then offered the full container bounds and
//! positioned according to the container-level `Alignment` (default: `CENTER`);
//! individual children can override alignment via `WidgetTree::set_alignment`.
//!
//! The primary use-cases are layered UIs — a background `RectWidget` beneath
//! a `TextWidget`, a floating badge over a button icon — and card-like
//! compositions where a paint layer and a content layer share the same bounds.
//! Children that expand to fill their proposal (e.g. `RectWidget`) fill the
//! full ZStack area; children with fixed intrinsic sizes are positioned by
//! alignment.
//!
//! Propagates shrink weight and minimum size when any child opts in, so
//! wrapping a shrinkable single-line label in a `ZStack` stays shrinkable.
//!
//! ```rust
//! # use bastyde_widgets::primitives::{ZStack, TextWidget};
//! # use bastyde_widgets::RectWidget;
//! # use bastyde_i18n::lit;
//! # use bastyde_tokens::SurfaceRole;
//! let _card = ZStack::new()
//!     .child(RectWidget::new().background(SurfaceRole::Raised))
//!     .child(TextWidget::new(lit!("Hello")));
//! ```

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
    /// Create an empty `ZStack` with center alignment.
    pub fn new() -> Self {
        Self {
            child_ids: Vec::new(),
            pending: Vec::new(),
            alignment: Alignment::CENTER,
        }
    }

    /// Set the alignment applied to every child that does not have a
    /// per-child override set via `WidgetTree::set_alignment`.
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
        // **Height gets a second, width-bounded query.** A height-for-width child
        // (wrapping text) measured at `unspecified()` has no basis for wrapping, so it
        // reports a *single line* — and the ZStack then sized its chrome to that, leaving
        // the real paragraph to paint outside its own background. Found in Skribisto,
        // where a toast body spilled over the status bar.
        //
        // The obvious fix — forwarding the whole incoming `proposal` — is the one this
        // code deliberately avoided, for two separate reasons, and both still stand:
        //
        //  * **Width.** `MinSize` forwards `Some(min)` as the width, so a *shrinkable*
        //    single-line label inside `MinSize → ZStack` would truncate to the min width
        //    during intrinsic measurement. `max_w` is therefore still taken only from the
        //    unspecified pass — untouched, bug-for-bug.
        //  * **Height.** Forwarding a bound `proposal.height` would let a greedy child
        //    (`RectWidget`) claim it and inflate the stack — exactly what the unspecified
        //    pass exists to prevent. So the second query pins `height: None` and offers
        //    only the width.
        //
        // What is left is precise: children are asked "how tall are you at the width you
        // will actually get?", and nothing else changes. A greedy background answers 0,
        // a truncating label answers one line either way, and only a wrapping child moves.
        let bounded_for_height = SizeProposal {
            width: proposal.width,
            height: None,
        };
        let query_twice = proposal.width.is_some();

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
            // Height only. Skipped entirely when the parent left the width open, since
            // the proposal would then be identical to the unspecified one above.
            if query_twice
                && let Some(r) = ctx.child_layout_response(child_id, bounded_for_height)
            {
                max_h = max_h.max(r.size.height);
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

    /// A child whose height depends on the width it is measured at — the shape of a
    /// wrapping paragraph, including its single-line answer when given no width.
    #[derive(Debug)]
    struct WrappingLeaf {
        natural_width: f32,
        line_height: f32,
    }
    impl Widget for WrappingLeaf {
        fn layout_response(
            &self,
            proposal: SizeProposal,
            _ctx: &LayoutContext,
        ) -> bastyde_core::widget::LayoutResponse {
            match proposal.width {
                Some(w) if w > 0.0 => {
                    let lines = (self.natural_width / w).ceil().max(1.0);
                    Size::new(w, lines * self.line_height).into()
                }
                _ => Size::new(self.natural_width, self.line_height).into(),
            }
        }
    }

    /// A child that claims whatever it is offered — `RectWidget`'s shape, and the reason
    /// the intrinsic pass exists.
    #[derive(Debug)]
    struct GreedyLeaf;
    impl Widget for GreedyLeaf {
        fn layout_response(
            &self,
            proposal: SizeProposal,
            _ctx: &LayoutContext,
        ) -> bastyde_core::widget::LayoutResponse {
            Size::new(
                proposal.width.unwrap_or(0.0),
                proposal.height.unwrap_or(0.0),
            )
            .into()
        }
    }

    /// The toast chrome's exact shape: a greedy background layered under wrapping content.
    ///
    /// Regression: both children were measured only at `unspecified()`, so the paragraph
    /// reported a single line and the ZStack sized its background to that — the remaining
    /// lines painted outside the chrome entirely. Seen in Skribisto as a toast body
    /// spilling over the status bar.
    #[test]
    fn a_wrapping_child_gets_its_real_height_when_the_width_is_bound() {
        let mut tree = WidgetTree::new();
        let bg = tree.add(GreedyLeaf);
        let text = tree.add(WrappingLeaf {
            natural_width: 400.0,
            line_height: 16.0,
        });
        let stack = tree.add(ZStack::new().add_child(bg).add_child(text));

        // Width bound, height open — what a toast surface is offered.
        tree.layout(SizeProposal {
            width: Some(100.0),
            height: None,
        });

        let b = tree.bounds(stack);
        assert!(
            (b.height - 64.0).abs() < 0.01,
            "400px of content at 100px wide is four 16px lines; got {} \
             (16 means the child was only ever measured unbounded)",
            b.height
        );
    }

    /// The guard the second query must not break: a greedy background still contributes
    /// nothing to the height, because that query pins `height: None`.
    #[test]
    fn a_greedy_background_still_does_not_inflate_the_stack() {
        let mut tree = WidgetTree::new();
        let bg = tree.add(GreedyLeaf);
        let content = tree.add(FixedLeaf(40.0, 20.0));
        let stack = tree.add(ZStack::new().add_child(bg).add_child(content));

        tree.layout(SizeProposal {
            width: Some(300.0),
            height: None,
        });

        let b = tree.bounds(stack);
        assert!(
            (b.height - 20.0).abs() < 0.01,
            "the background must not set the height; got {}",
            b.height
        );
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
