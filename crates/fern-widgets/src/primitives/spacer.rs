use fern_canvas::{Rect, Size, SizeProposal};
use fern_core::widget::{LayoutContext, PaintContext, Widget};

/// A flexible spacer that claims all available space on the container's
/// primary axis. Place a Spacer in an HStack or VStack to push siblings
/// to the edges.
#[derive(Debug)]
pub struct Spacer {
    min_length: f32,
}

impl Spacer {
    pub fn new() -> Self {
        Self { min_length: 0.0 }
    }

    pub fn min_length(mut self, min: f32) -> Self {
        self.min_length = min;
        self
    }
}

impl Default for Spacer {
    fn default() -> Self {
        Self::new()
    }
}

impl Widget for Spacer {
    fn size_that_fits(&self, proposal: SizeProposal, _ctx: &LayoutContext) -> Size {
        // Claim all offered space; fall back to min_length if unspecified.
        proposal.resolve(self.min_length, self.min_length)
    }

    fn paint(&self, _bounds: Rect, _canvas: &mut fern_canvas::Canvas, _ctx: &PaintContext) {
        // Spacer is invisible.
    }

    fn is_spacer(&self) -> bool {
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use fern_core::widget_tree::WidgetTree;

    use crate::primitives::hstack::HStack;
    use crate::primitives::vstack::VStack;

    #[derive(Debug)]
    struct FixedLeaf(f32, f32);
    impl Widget for FixedLeaf {
        fn size_that_fits(&self, _proposal: SizeProposal, _ctx: &LayoutContext) -> Size {
            Size::new(self.0, self.1)
        }
    }

    #[test]
    fn spacer_pushes_to_trailing_in_hstack() {
        let mut tree = WidgetTree::new();
        let spacer = tree.add(Spacer::new());
        let btn = tree.add(FixedLeaf(60.0, 30.0));
        let _stack = tree.add(HStack::new().add_child(spacer).add_child(btn));
        tree.layout(SizeProposal::exact(300.0, 50.0));

        // Spacer takes 300-60=240, button at x=240
        assert!((tree.bounds(btn).x - 240.0).abs() < 0.01);
    }

    #[test]
    fn two_spacers_center_child_in_hstack() {
        let mut tree = WidgetTree::new();
        let s1 = tree.add(Spacer::new());
        let label = tree.add(FixedLeaf(60.0, 30.0));
        let s2 = tree.add(Spacer::new());
        let _stack = tree.add(HStack::new().add_child(s1).add_child(label).add_child(s2));
        tree.layout(SizeProposal::exact(300.0, 50.0));

        // Remaining = 300-60 = 240, each spacer = 120
        // label at x=120
        assert!((tree.bounds(label).x - 120.0).abs() < 0.01);
    }

    #[test]
    fn spacer_pushes_to_bottom_in_vstack() {
        let mut tree = WidgetTree::new();
        let spacer = tree.add(Spacer::new());
        let btn = tree.add(FixedLeaf(60.0, 30.0));
        let _stack = tree.add(VStack::new().add_child(spacer).add_child(btn));
        tree.layout(SizeProposal::exact(200.0, 300.0));

        // Spacer takes 300-30=270, button at y=270
        assert!((tree.bounds(btn).y - 270.0).abs() < 0.01);
    }

    #[test]
    fn spacer_with_min_length() {
        let mut tree = WidgetTree::new();
        let btn1 = tree.add(FixedLeaf(60.0, 30.0));
        let spacer = tree.add(Spacer::new().min_length(20.0));
        let btn2 = tree.add(FixedLeaf(60.0, 30.0));
        let _stack = tree.add(
            HStack::new()
                .add_child(btn1)
                .add_child(spacer)
                .add_child(btn2),
        );
        tree.layout(SizeProposal::exact(300.0, 50.0));

        // Spacer gets 300-60-60 = 180 (well above min_length)
        assert!((tree.bounds(btn2).x - 240.0).abs() < 0.01);
    }

    #[test]
    fn is_spacer_returns_true() {
        assert!(Spacer::new().is_spacer());
    }
}
