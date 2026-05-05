//! AspectRatio — a single-child wrapper that constrains layout to a fixed
//! width-to-height ratio.

use fern_canvas::{Point, Rect, Size, SizeProposal};
use fern_core::accessibility::AccessNodeBuilder;
use fern_core::widget::{LayoutContext, PaintContext, PendingChild, Widget, WidgetPlacement};
use fern_core::widget_id::WidgetId;

/// A single-child wrapper that maintains a fixed width/height ratio.
#[derive(Debug)]
pub struct AspectRatio {
    /// Width divided by height (e.g., 16.0/9.0 for widescreen).
    ratio: f32,
    child_id: Option<WidgetId>,
    pending_child: Option<PendingChild>,
}

impl AspectRatio {
    /// Create a new aspect ratio wrapper. Ratio is width / height.
    pub fn new(ratio: f32) -> Self {
        Self {
            ratio: ratio.max(f32::EPSILON),
            child_id: None,
            pending_child: None,
        }
    }

    /// Convenience for 16:9 aspect ratio.
    pub fn widescreen() -> Self {
        Self::new(16.0 / 9.0)
    }

    /// Convenience for 1:1 aspect ratio.
    pub fn square() -> Self {
        Self::new(1.0)
    }

    pub fn child(mut self, widget: impl Widget + 'static) -> Self {
        self.pending_child = Some(PendingChild::Deferred(Box::new(widget)));
        self
    }

    pub fn child_id(mut self, id: WidgetId) -> Self {
        self.pending_child = Some(PendingChild::Id(id));
        self
    }

    /// Given available space, compute the largest size that fits the ratio.
    fn constrain(&self, width: Option<f32>, height: Option<f32>) -> Size {
        match (width, height) {
            (Some(w), Some(h)) => {
                // Fit within both constraints
                let h_from_w = w / self.ratio;
                let w_from_h = h * self.ratio;
                if h_from_w <= h {
                    Size::new(w, h_from_w)
                } else {
                    Size::new(w_from_h, h)
                }
            }
            (Some(w), None) => Size::new(w, w / self.ratio),
            (None, Some(h)) => Size::new(h * self.ratio, h),
            (None, None) => Size::new(0.0, 0.0),
        }
    }
}

impl Widget for AspectRatio {
    fn build(&mut self, ctx: &mut fern_core::build_context::BuildContext) -> Vec<WidgetId> {
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
        _ctx: &LayoutContext,
    ) -> fern_core::widget::LayoutResponse {
        self.constrain(proposal.width, proposal.height).into()
    }

    fn place_children(
        &self,
        bounds: Rect,
        _proposal: SizeProposal,
        children: &mut [WidgetPlacement],
        _ctx: &LayoutContext,
    ) {
        for child in children.iter_mut() {
            child.origin = Point::new(bounds.x, bounds.y);
            child.size = Size::new(bounds.width, bounds.height);
        }
    }

    fn paint(&self, _bounds: Rect, _canvas: &mut fern_canvas::Canvas, _ctx: &PaintContext) {}

    fn accessibility(&self, builder: &mut AccessNodeBuilder) {
        builder.set_hidden();
    }

    fn children(&self) -> Vec<WidgetId> {
        self.child_id.into_iter().collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use fern_core::widget_tree::WidgetTree;

    #[derive(Debug)]
    struct FixedLeaf(f32, f32);
    impl Widget for FixedLeaf {
        fn layout_response(
            &self,
            _proposal: SizeProposal,
            _ctx: &LayoutContext,
        ) -> fern_core::widget::LayoutResponse {
            Size::new(self.0, self.1).into()
        }
    }

    #[test]
    fn aspect_ratio_constrains_by_width() {
        let mut tree = WidgetTree::new();
        let child = tree.add(FixedLeaf(100.0, 100.0));
        let ar = tree.add(AspectRatio::new(2.0).child_id(child)); // 2:1
        tree.layout(SizeProposal {
            width: Some(200.0),
            height: None,
        });

        let b = tree.bounds(ar);
        assert!((b.width - 200.0).abs() < 0.01);
        assert!((b.height - 100.0).abs() < 0.01); // 200/2 = 100
    }

    #[test]
    fn aspect_ratio_constrains_by_height() {
        let mut tree = WidgetTree::new();
        let child = tree.add(FixedLeaf(100.0, 100.0));
        let ar = tree.add(AspectRatio::new(2.0).child_id(child)); // 2:1
        tree.layout(SizeProposal {
            width: None,
            height: Some(100.0),
        });

        let b = tree.bounds(ar);
        // height=100, width_from_height = 100*2 = 200
        // width_from_width = 400, height = 400/2 = 200 > 100
        // So constrain by height: 200x100
        assert!((b.width - 200.0).abs() < 0.01);
        assert!((b.height - 100.0).abs() < 0.01);
    }

    #[test]
    fn square_aspect_ratio() {
        let mut tree = WidgetTree::new();
        let child = tree.add(FixedLeaf(50.0, 50.0));
        let ar = tree.add(AspectRatio::square().child_id(child));
        tree.layout(SizeProposal {
            width: None,
            height: Some(100.0),
        });

        let b = tree.bounds(ar);
        assert!((b.width - 100.0).abs() < 0.01);
        assert!((b.height - 100.0).abs() < 0.01);
    }

    #[test]
    fn width_only_proposal() {
        let ar = AspectRatio::new(4.0 / 3.0);
        let theme = fern_tokens::Theme::light_default();
        let ctx = LayoutContext::for_testing(&theme);
        let size = ar
            .layout_response(
                SizeProposal {
                    width: Some(400.0),
                    height: None,
                },
                &ctx,
            )
            .size;
        assert!((size.width - 400.0).abs() < 0.01);
        assert!((size.height - 300.0).abs() < 0.01); // 400 / (4/3)
    }
}
