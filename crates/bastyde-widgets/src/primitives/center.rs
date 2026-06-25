// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

use bastyde_canvas::{Point, Rect, Size, SizeProposal};
use bastyde_core::accessibility::AccessNodeBuilder;
use bastyde_core::widget::{LayoutContext, PaintContext, PendingChild, Widget, WidgetPlacement};
use bastyde_core::widget_id::WidgetId;

/// Centers a single child within the space this widget is **given**.
///
/// Sizing follows the incoming constraint, per axis: `Center` **fills a
/// bounded axis** (the tree root, or inside an `Expand` / wrapper that
/// proposes exact bounds) and **shrink-wraps to the child on an unbounded
/// axis**. So a bare `Center` does *not* claim slack inside an `HStack` /
/// `VStack` — those leave their main axis open, and `Center` sizes to its
/// child there (like Flutter's `Center` / `Align`, or Compose's `Box`),
/// rather than collapsing to zero and letting the child overflow.
///
/// Centering and *expanding* are separate concerns: `Center` reports
/// `flex = 0` and is a pure alignment wrapper, never a space-claiming one. To
/// center a child *within the leftover space* of a stack, give it flex with
/// `Expand` — `Expand::horizontal { Center { child } }` (the analogue of
/// Flutter's `Expanded(child: Center(...))`).
#[derive(Debug)]
pub struct Center {
    child_id: Option<WidgetId>,
    pending_child: Option<PendingChild>,
}

impl Center {
    pub fn new() -> Self {
        Self {
            child_id: None,
            pending_child: None,
        }
    }

    /// Set child by pre-registered ID.
    pub fn child_id(mut self, id: WidgetId) -> Self {
        self.pending_child = Some(PendingChild::Id(id));
        self
    }

    /// Set an inline child widget (deferred insertion).
    pub fn child(mut self, widget: impl Widget + 'static) -> Self {
        self.pending_child = Some(PendingChild::Deferred(Box::new(widget)));
        self
    }
}

impl Default for Center {
    fn default() -> Self {
        Self::new()
    }
}

impl Widget for Center {
    fn build(&mut self, ctx: &mut bastyde_core::build_context::BuildContext) -> Vec<WidgetId> {
        if let Some(pending) = self.pending_child.take() {
            self.child_id = Some(match pending {
                PendingChild::Id(id) => id,
                PendingChild::Deferred(w) => ctx.add_boxed(w),
            });
        }
        self.child_id.into_iter().collect()
    }

    fn layout_response(
        &self,
        proposal: SizeProposal,
        ctx: &LayoutContext,
    ) -> bastyde_core::widget::LayoutResponse {
        // Fill a bounded axis; shrink-wrap to the child on an unbounded one.
        // A stack leaves its main axis open (`None`) when querying children, so
        // resolving that to `0` (the old behavior) made `Center` collapse to
        // zero width/height there and its child overflowed. Sizing to the child
        // instead keeps `Center` a well-behaved, non-greedy alignment wrapper
        // (`flex = 0`): it occupies its child on the open axis and fills only
        // axes the parent actually bounded.
        let child = self
            .child_id
            .and_then(|id| ctx.child_size(id, SizeProposal::unspecified()))
            .unwrap_or(Size::ZERO);
        Size::new(
            proposal.width.unwrap_or(child.width),
            proposal.height.unwrap_or(child.height),
        )
        .into()
    }

    fn place_children(
        &self,
        bounds: Rect,
        _proposal: SizeProposal,
        children: &mut [WidgetPlacement],
        ctx: &LayoutContext,
    ) {
        for child in children.iter_mut() {
            let child_size = ctx
                .child_size(child.id, SizeProposal::unspecified())
                .unwrap_or(bounds.size());
            let dx = (bounds.width - child_size.width) / 2.0;
            let dy = (bounds.height - child_size.height) / 2.0;
            child.origin = Point::new(bounds.x + dx, bounds.y + dy);
            child.size = child_size;
        }
    }

    fn paint(&self, _bounds: Rect, _canvas: &mut bastyde_canvas::Canvas, _ctx: &PaintContext) {}

    fn accessibility(&self, builder: &mut AccessNodeBuilder) {
        builder.set_role(bastyde_core::accesskit::Role::GenericContainer);
    }

    fn children(&self) -> Vec<WidgetId> {
        self.child_id.into_iter().collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bastyde_canvas::Size;
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
    fn centers_child() {
        let mut tree = WidgetTree::new();
        let child = tree.add(FixedLeaf(40.0, 20.0));
        let _center = tree.add(Center::new().child_id(child));
        tree.layout(SizeProposal::exact(200.0, 100.0));

        let cb = tree.bounds(child);
        assert!((cb.x - 80.0).abs() < 0.01); // (200-40)/2
        assert!((cb.y - 40.0).abs() < 0.01); // (100-20)/2
    }

    #[test]
    fn claims_full_space() {
        let mut tree = WidgetTree::new();
        let child = tree.add(FixedLeaf(40.0, 20.0));
        let center = tree.add(Center::new().child_id(child));
        tree.layout(SizeProposal::exact(200.0, 100.0));

        let cb = tree.bounds(center);
        assert!((cb.width - 200.0).abs() < 0.01);
        assert!((cb.height - 100.0).abs() < 0.01);
    }

    /// Regression: a bare `Center` inside an `HStack` must **shrink-wrap its
    /// child**, not collapse to zero width and let the child overflow left over
    /// a prior sibling. (Old `proposal.resolve(0,0)` yielded width 0, so the
    /// child was "centered" around x=0 and overlapped the logo.) Also asserts
    /// the conventional escape — `Expand { Center { .. } }` — claims the slack
    /// and centers within it.
    #[test]
    fn center_in_hstack_shrink_wraps_child_without_overflow() {
        use crate::primitives::{Expand, HStack};

        // Bare Center: HStack { Fixed(50), Center { Fixed(40) } } in 300px.
        let mut tree = WidgetTree::new();
        let logo = tree.add(FixedLeaf(50.0, 20.0));
        let title = tree.add(FixedLeaf(40.0, 20.0));
        let center = tree.add(Center::new().child_id(title));
        let _row = tree.add(HStack::new().add_child(logo).add_child(center));
        tree.layout(SizeProposal::exact(300.0, 20.0));

        assert!(
            (tree.bounds(center).width - 40.0).abs() < 0.01,
            "Center should shrink-wrap its child (40), got {}",
            tree.bounds(center).width
        );
        let logo_right = tree.bounds(logo).x + tree.bounds(logo).width;
        assert!(
            tree.bounds(title).x >= logo_right - 0.01,
            "title must not overflow left over the logo: title.x={}, logo right={}",
            tree.bounds(title).x,
            logo_right
        );

        // Conventional fill+center: Expand claims the 250 slack, Center fills it
        // and centers the 40px title → title.x = 50 + (250-40)/2 = 155.
        let mut t2 = WidgetTree::new();
        let logo2 = t2.add(FixedLeaf(50.0, 20.0));
        let title2 = t2.add(FixedLeaf(40.0, 20.0));
        let center2 = t2.add(Center::new().child_id(title2));
        let exp = t2.add(Expand::horizontal().child_id(center2));
        let _row2 = t2.add(HStack::new().add_child(logo2).add_child(exp));
        t2.layout(SizeProposal::exact(300.0, 20.0));

        assert!(
            (t2.bounds(exp).width - 250.0).abs() < 0.01,
            "Expand should claim the slack (250), got {}",
            t2.bounds(exp).width
        );
        assert!(
            (t2.bounds(title2).x - 155.0).abs() < 0.5,
            "title should be centered in the remaining space (~155), got {}",
            t2.bounds(title2).x
        );
    }
}
