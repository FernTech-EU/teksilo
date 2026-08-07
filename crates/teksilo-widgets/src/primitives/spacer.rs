// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Spacer — an invisible, flexible gap that claims all available space on the
//! container's main axis.
//!
//! Place a `Spacer` inside an [`HStack`](crate::primitives::HStack) or
//! [`VStack`](crate::primitives::VStack) to push adjacent siblings to opposite
//! edges; flank a child with two spacers to centre it. A spacer carries flex
//! weight `1.0` and zero wanted size, so it soaks up leftover slack without
//! imposing a cross-axis floor. [`min_length`](Spacer::min_length) sets a hard
//! minimum so the gap never collapses below a fixed amount under tight layout.
//!
//! ```rust
//! # use teksilo_widgets::primitives::{HStack, Spacer, TextWidget};
//! # use teksilo_i18n::lit;
//! // Title hugs the leading edge, badge is pushed to the trailing edge.
//! let _row = HStack::new()
//!     .child(TextWidget::new(lit!("Title")))
//!     .child(Spacer::new())
//!     .child(TextWidget::new(lit!("NEW")));
//! ```

use teksilo_canvas::{Rect, Size, SizeProposal};
use teksilo_core::widget::{LayoutContext, PaintContext, Widget};

/// An invisible, flexible gap that claims a container's leftover main-axis space.
#[derive(Debug)]
pub struct Spacer {
    min_length: f32,
}

impl Spacer {
    /// Create a spacer with no minimum length (collapses fully when the
    /// container has no slack to give).
    pub fn new() -> Self {
        Self { min_length: 0.0 }
    }

    /// Set a hard floor, in logical pixels, on the spacer's main-axis size.
    ///
    /// The container still adds its slack share on top; the floor only matters
    /// when the container is too cramped to grant any slack. The cross axis is
    /// unaffected, so a horizontal spacer never inflates its stack's height.
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
    fn layout_response(
        &self,
        _proposal: SizeProposal,
        ctx: &LayoutContext,
    ) -> teksilo_core::widget::LayoutResponse {
        // Spacer wants `min_length` as a floor on the stack's MAIN axis; the
        // parent adds its slack share on top via the flex weight. The cross
        // axis must be 0 — otherwise an invisible spacer with `min_length > 0`
        // imposes a spurious cross-axis floor on the stack (e.g. an HStack's
        // intrinsic height grows by `min_length`). The enclosing stack tells us
        // its main axis via the context; outside a stack we fall back to
        // `min_length` on both axes (a spacer there is degenerate anyway).
        use teksilo_core::widget::StackAxis;
        let size = match ctx.stack_main_axis() {
            Some(StackAxis::Horizontal) => Size::new(self.min_length, 0.0),
            Some(StackAxis::Vertical) => Size::new(0.0, self.min_length),
            None => Size::new(self.min_length, self.min_length),
        };
        teksilo_core::widget::LayoutResponse::flexible(size, 1.0)
    }

    fn paint(&self, _bounds: Rect, _canvas: &mut teksilo_canvas::Canvas, _ctx: &PaintContext) {
        // Spacer is invisible.
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use teksilo_core::widget_tree::WidgetTree;

    use crate::primitives::hstack::HStack;
    use crate::primitives::vstack::VStack;

    #[derive(Debug)]
    struct FixedLeaf(f32, f32);
    impl Widget for FixedLeaf {
        fn layout_response(
            &self,
            _proposal: SizeProposal,
            _ctx: &LayoutContext,
        ) -> teksilo_core::widget::LayoutResponse {
            Size::new(self.0, self.1).into()
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
    fn min_length_does_not_inflate_cross_axis() {
        // Regression: a horizontal Spacer with `min_length` must not inflate
        // its HStack's intrinsic height. The HStack is measured with an open
        // (intrinsic) height; only the real content (30px) should drive it.
        let mut tree = WidgetTree::new();
        let btn = tree.add(FixedLeaf(60.0, 30.0));
        let spacer = tree.add(Spacer::new().min_length(80.0));
        let stack = tree.add(HStack::new().add_child(btn).add_child(spacer));
        // Width fixed, height open → intrinsic height.
        tree.layout(SizeProposal {
            width: Some(400.0),
            height: None,
        });
        assert!(
            (tree.bounds(stack).height - 30.0).abs() < 0.01,
            "HStack height should follow content (30), not the spacer min_length (80); got {}",
            tree.bounds(stack).height
        );
    }

    #[test]
    fn min_length_is_honoured_on_the_main_axis() {
        // The main-axis floor still holds: a cramped HStack keeps the spacer at
        // least `min_length` wide.
        let mut tree = WidgetTree::new();
        let btn1 = tree.add(FixedLeaf(60.0, 30.0));
        let spacer = tree.add(Spacer::new().min_length(40.0));
        let btn2 = tree.add(FixedLeaf(60.0, 30.0));
        let stack = tree.add(
            HStack::new()
                .add_child(btn1)
                .add_child(spacer)
                .add_child(btn2),
        );
        // 60 + 40 + 60 = 160 exactly → spacer at its floor, btn2 at x=100.
        tree.layout(SizeProposal::exact(160.0, 50.0));
        assert!((tree.bounds(btn2).x - 100.0).abs() < 0.01);
        let _ = stack;
    }

    #[test]
    fn flex_factor_is_one() {
        let theme = teksilo_core::presets::intui::light();
        let ctx = LayoutContext::for_testing(&theme);
        let r = Spacer::new().layout_response(SizeProposal::unspecified(), &ctx);
        assert_eq!(r.flex, 1.0);
    }
}
