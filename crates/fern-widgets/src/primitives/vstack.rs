use fern_canvas::{Point, Rect, Size, SizeProposal};
use fern_core::widget::{LayoutContext, PaintContext, Widget, WidgetPlacement};
use fern_core::widget_id::WidgetId;
use fern_tokens::HAlignment;

/// Vertical layout container that distributes children top-to-bottom
/// based on their intrinsic sizes. Cross-axis alignment is controlled
/// by `HAlignment` (default: `Leading`).
#[derive(Debug)]
pub struct VStack {
    child_ids: Vec<WidgetId>,
    spacing: f32,
    alignment: HAlignment,
}

impl VStack {
    pub fn new() -> Self {
        Self {
            child_ids: Vec::new(),
            spacing: 0.0,
            alignment: HAlignment::Leading,
        }
    }

    pub fn spacing(mut self, spacing: f32) -> Self {
        self.spacing = spacing;
        self
    }

    pub fn alignment(mut self, alignment: HAlignment) -> Self {
        self.alignment = alignment;
        self
    }

    pub fn add_child(mut self, id: WidgetId) -> Self {
        self.child_ids.push(id);
        self
    }
}

impl Default for VStack {
    fn default() -> Self {
        Self::new()
    }
}

impl Widget for VStack {
    fn size_that_fits(&self, proposal: SizeProposal, ctx: &LayoutContext) -> Size {
        if self.child_ids.is_empty() {
            return proposal.resolve(0.0, 0.0);
        }

        // Query each child's intrinsic size: height=None (ideal), width from proposal
        let child_proposal = SizeProposal {
            width: proposal.width,
            height: None,
        };

        let mut total_height: f32 = 0.0;
        let mut max_width: f32 = 0.0;

        for &child_id in &self.child_ids {
            if ctx.child_is_spacer(child_id) {
                continue; // spacers don't contribute intrinsic height
            }
            if let Some(child_size) = ctx.child_size(child_id, child_proposal) {
                total_height += child_size.height;
                max_width = max_width.max(child_size.width);
            }
        }

        let n = self.child_ids.len();
        let total_spacing = self.spacing * (n as f32 - 1.0).max(0.0);
        total_height += total_spacing;

        let width = proposal.width.unwrap_or(max_width);
        let height = proposal.height.unwrap_or(total_height);

        Size::new(width, height)
    }

    fn place_children(
        &self,
        bounds: Rect,
        _proposal: SizeProposal,
        children: &mut [WidgetPlacement],
        ctx: &LayoutContext,
    ) {
        let n = children.len();
        if n == 0 {
            return;
        }

        // Query each child's intrinsic size once with unspecified proposal.
        let intrinsic_proposal = SizeProposal::unspecified();

        let mut intrinsic_widths: Vec<f32> = Vec::with_capacity(n);
        let mut intrinsic_heights: Vec<f32> = Vec::with_capacity(n);
        let mut is_spacer: Vec<bool> = Vec::with_capacity(n);
        let mut total_non_spacer_height: f32 = 0.0;
        let mut spacer_count = 0;

        for child in children.iter() {
            let spacer = ctx.child_is_spacer(child.id);
            is_spacer.push(spacer);
            if spacer {
                intrinsic_widths.push(bounds.width);
                intrinsic_heights.push(0.0);
                spacer_count += 1;
            } else {
                let size = ctx
                    .child_size(child.id, intrinsic_proposal)
                    .unwrap_or(Size::ZERO);
                intrinsic_widths.push(size.width);
                intrinsic_heights.push(size.height);
                total_non_spacer_height += size.height;
            }
        }

        // Distribute remaining space among spacers
        let total_spacing = self.spacing * (n as f32 - 1.0).max(0.0);
        let remaining = (bounds.height - total_non_spacer_height - total_spacing).max(0.0);
        let spacer_height = if spacer_count > 0 {
            remaining / spacer_count as f32
        } else {
            0.0
        };

        // Place children top-to-bottom with alignment on cross axis
        let rtl = ctx.is_rtl();
        let mut y = bounds.y;
        for (i, child) in children.iter_mut().enumerate() {
            let w = intrinsic_widths[i];
            let h = if is_spacer[i] {
                spacer_height
            } else {
                intrinsic_heights[i]
            };

            // Cross-axis alignment: check per-child override, then container default
            let halign = ctx
                .child_alignment(child.id)
                .map(|a| a.horizontal)
                .unwrap_or(self.alignment);
            let x_offset = halign.resolve(w, bounds.width, rtl);

            child.origin = Point::new(bounds.x + x_offset, y);
            child.size = Size::new(w, h);
            y += h + self.spacing;
        }
    }

    fn paint(&self, _bounds: Rect, _canvas: &mut fern_canvas::Canvas, _ctx: &PaintContext) {}

    fn children(&self) -> Vec<WidgetId> {
        self.child_ids.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use fern_core::widget_tree::WidgetTree;

    /// A leaf that always reports a fixed intrinsic size.
    #[derive(Debug)]
    struct FixedLeaf(f32, f32);
    impl Widget for FixedLeaf {
        fn size_that_fits(&self, _proposal: SizeProposal, _ctx: &LayoutContext) -> Size {
            Size::new(self.0, self.1)
        }
    }

    #[test]
    fn children_get_intrinsic_heights() {
        let mut tree = WidgetTree::new();
        let a = tree.add(FixedLeaf(80.0, 30.0));
        let b = tree.add(FixedLeaf(60.0, 50.0));
        let _stack = tree.add(VStack::new().add_child(a).add_child(b));
        tree.layout(SizeProposal::exact(200.0, 300.0));

        assert!((tree.bounds(a).height - 30.0).abs() < 0.01);
        assert!((tree.bounds(b).height - 50.0).abs() < 0.01);
        assert!((tree.bounds(b).y - 30.0).abs() < 0.01);
    }

    #[test]
    fn spacing_between_children() {
        let mut tree = WidgetTree::new();
        let a = tree.add(FixedLeaf(80.0, 40.0));
        let b = tree.add(FixedLeaf(80.0, 40.0));
        let _stack = tree.add(VStack::new().spacing(10.0).add_child(a).add_child(b));
        tree.layout(SizeProposal::exact(200.0, 300.0));

        assert!((tree.bounds(b).y - 50.0).abs() < 0.01); // 40 + 10
    }

    #[test]
    fn cross_axis_leading_alignment_ltr() {
        let mut tree = WidgetTree::new();
        let a = tree.add(FixedLeaf(80.0, 30.0));
        let _stack = tree.add(VStack::new().add_child(a)); // default: Leading
        tree.layout(SizeProposal::exact(200.0, 300.0));

        assert!((tree.bounds(a).x - 0.0).abs() < 0.01); // Leading = left in LTR
    }

    #[test]
    fn cross_axis_center_alignment() {
        let mut tree = WidgetTree::new();
        let a = tree.add(FixedLeaf(80.0, 30.0));
        let _stack = tree.add(VStack::new().alignment(HAlignment::Center).add_child(a));
        tree.layout(SizeProposal::exact(200.0, 300.0));

        assert!((tree.bounds(a).x - 60.0).abs() < 0.01); // (200-80)/2
    }

    #[test]
    fn cross_axis_trailing_alignment() {
        let mut tree = WidgetTree::new();
        let a = tree.add(FixedLeaf(80.0, 30.0));
        let _stack = tree.add(VStack::new().alignment(HAlignment::Trailing).add_child(a));
        tree.layout(SizeProposal::exact(200.0, 300.0));

        assert!((tree.bounds(a).x - 120.0).abs() < 0.01); // 200 - 80
    }

    #[test]
    fn per_child_alignment_override() {
        let mut tree = WidgetTree::new();
        let a = tree.add(FixedLeaf(80.0, 30.0));
        let b = tree.add(FixedLeaf(60.0, 30.0));
        let _stack = tree.add(VStack::new().add_child(a).add_child(b)); // default: Leading
        tree.set_alignment(
            b,
            fern_tokens::Alignment {
                horizontal: fern_tokens::HAlignment::Trailing,
                vertical: fern_tokens::VAlignment::Center,
            },
        );
        tree.layout(SizeProposal::exact(200.0, 300.0));

        assert!((tree.bounds(a).x - 0.0).abs() < 0.01); // Leading
        assert!((tree.bounds(b).x - 140.0).abs() < 0.01); // Trailing: 200-60
    }

    #[test]
    fn empty_vstack() {
        let mut tree = WidgetTree::new();
        let _stack = tree.add(VStack::new());
        tree.layout(SizeProposal::exact(200.0, 50.0));
    }
}
