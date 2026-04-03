use fern_canvas::{Point, Rect, Size, SizeProposal};
use fern_core::accessibility::AccessNodeBuilder;
use fern_core::state::{Reactive, State};
use fern_core::widget::{IntoWidgetTree, LayoutContext, PaintContext, PendingChild, Widget, WidgetPlacement};
use fern_core::widget_id::WidgetId;
use fern_tokens::VAlignment;

/// Horizontal layout container that distributes children left-to-right
/// based on their intrinsic sizes. Cross-axis alignment is controlled
/// by `VAlignment` (default: `Center`).
#[derive(Debug)]
pub struct HStack {
    child_ids: Vec<WidgetId>,
    pending: Vec<PendingChild>,
    spacing: f32,
    alignment: VAlignment,
    visible_when_state: Option<Reactive<bool>>,
    enabled_when_state: Option<Reactive<bool>>,
}

impl HStack {
    pub fn new() -> Self {
        Self {
            child_ids: Vec::new(),
            pending: Vec::new(),
            spacing: 0.0,
            alignment: VAlignment::Center,
            visible_when_state: None,
            enabled_when_state: None,
        }
    }

    pub fn spacing(mut self, spacing: f32) -> Self {
        self.spacing = spacing;
        self
    }

    pub fn alignment(mut self, alignment: VAlignment) -> Self {
        self.alignment = alignment;
        self
    }

    /// Add a pre-registered child by ID.
    pub fn add_child(mut self, id: WidgetId) -> Self {
        self.pending.push(PendingChild::Id(id));
        self
    }

    /// Add an inline child widget (deferred insertion).
    pub fn child(mut self, widget: impl IntoWidgetTree) -> Self {
        self.pending.push(PendingChild::Deferred(Box::new(widget)));
        self
    }

    /// Add multiple inline children from an iterator.
    pub fn children(
        mut self,
        iter: impl IntoIterator<Item = impl IntoWidgetTree>,
    ) -> Self {
        for widget in iter {
            self.pending.push(PendingChild::Deferred(Box::new(widget)));
        }
        self
    }

    /// Conditionally add a child. No-op if None.
    pub fn child_opt(mut self, widget: Option<impl IntoWidgetTree>) -> Self {
        if let Some(w) = widget {
            self.pending.push(PendingChild::Deferred(Box::new(w)));
        }
        self
    }

    /// Bind visibility to a boolean state (toggles dormant/active).
    pub fn visible_when(mut self, state: impl Into<Reactive<bool>>) -> Self {
        self.visible_when_state = Some(state.into());
        self
    }

    /// Bind enabled state to a boolean state.
    pub fn enabled_when(mut self, state: impl Into<Reactive<bool>>) -> Self {
        self.enabled_when_state = Some(state.into());
        self
    }
}

impl Default for HStack {
    fn default() -> Self {
        Self::new()
    }
}

impl Widget for HStack {
    fn size_that_fits(&self, proposal: SizeProposal, ctx: &LayoutContext) -> Size {
        if self.child_ids.is_empty() {
            return proposal.resolve(0.0, 0.0);
        }

        // Query each child's intrinsic size: width=None (ideal), height from proposal
        let child_proposal = SizeProposal {
            width: None,
            height: proposal.height,
        };

        let mut total_width: f32 = 0.0;
        let mut max_height: f32 = 0.0;
        let mut non_spacer_count = 0;

        for &child_id in &self.child_ids {
            if ctx.child_is_spacer(child_id) {
                continue; // spacers don't contribute intrinsic width
            }
            if let Some(child_size) = ctx.child_size(child_id, child_proposal) {
                total_width += child_size.width;
                max_height = max_height.max(child_size.height);
                non_spacer_count += 1;
            }
        }

        let n = self.child_ids.len();
        let total_spacing = self.spacing * (n as f32 - 1.0).max(0.0);
        total_width += total_spacing;

        // Primary axis: fill the offered width only when spacers need to expand.
        // Without spacers, use the intrinsic content width.
        let has_spacers = self.child_ids.iter().any(|&id| ctx.child_is_spacer(id));
        let width = if has_spacers {
            proposal.width.unwrap_or(total_width)
        } else {
            total_width
        };
        let height = proposal.height.unwrap_or(max_height);

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

        // Query each child's intrinsic size: offer the full container height
        // so children containing spacers (e.g. VStack) can expand properly.
        let intrinsic_proposal = SizeProposal {
            width: None,
            height: Some(bounds.height),
        };

        let mut intrinsic_widths: Vec<f32> = Vec::with_capacity(n);
        let mut intrinsic_heights: Vec<f32> = Vec::with_capacity(n);
        let mut is_spacer: Vec<bool> = Vec::with_capacity(n);
        let mut total_non_spacer_width: f32 = 0.0;
        let mut spacer_count = 0;

        for child in children.iter() {
            let spacer = ctx.child_is_spacer(child.id);
            is_spacer.push(spacer);
            if spacer {
                intrinsic_widths.push(0.0);
                intrinsic_heights.push(bounds.height);
                spacer_count += 1;
            } else {
                let size = ctx
                    .child_size(child.id, intrinsic_proposal)
                    .unwrap_or(Size::ZERO);
                intrinsic_widths.push(size.width);
                intrinsic_heights.push(size.height);
                total_non_spacer_width += size.width;
            }
        }

        // Distribute remaining space among spacers
        let total_spacing = self.spacing * (n as f32 - 1.0).max(0.0);
        let remaining = (bounds.width - total_non_spacer_width - total_spacing).max(0.0);
        let spacer_width = if spacer_count > 0 {
            remaining / spacer_count as f32
        } else {
            0.0
        };

        // Place children with alignment on cross axis.
        // In RTL mode, children are placed right-to-left.
        let rtl = ctx.is_rtl();
        if rtl {
            let mut x = bounds.right();
            for (i, child) in children.iter_mut().enumerate() {
                let w = if is_spacer[i] {
                    spacer_width
                } else {
                    intrinsic_widths[i]
                };
                let h = intrinsic_heights[i];
                let valign = ctx
                    .child_alignment(child.id)
                    .map(|a| a.vertical)
                    .unwrap_or(self.alignment);
                let y_offset = valign.resolve(h, bounds.height);

                x -= w;
                child.origin = Point::new(x, bounds.y + y_offset);
                child.size = Size::new(w, h);
                x -= self.spacing;
            }
        } else {
            let mut x = bounds.x;
            for (i, child) in children.iter_mut().enumerate() {
                let w = if is_spacer[i] {
                    spacer_width
                } else {
                    intrinsic_widths[i]
                };
                let h = intrinsic_heights[i];
                let valign = ctx
                    .child_alignment(child.id)
                    .map(|a| a.vertical)
                    .unwrap_or(self.alignment);
                let y_offset = valign.resolve(h, bounds.height);

                child.origin = Point::new(x, bounds.y + y_offset);
                child.size = Size::new(w, h);
                x += w + self.spacing;
            }
        }
    }

    fn paint(&self, _bounds: Rect, _canvas: &mut fern_canvas::Canvas, _ctx: &PaintContext) {}

    fn accessibility(&self, builder: &mut AccessNodeBuilder) {
        builder.set_role(fern_core::accesskit::Role::GenericContainer);
    }

    fn children(&self) -> Vec<WidgetId> {
        self.child_ids.clone()
    }

    fn take_pending_children(&mut self) -> Vec<PendingChild> {
        std::mem::take(&mut self.pending)
    }

    fn set_resolved_children(&mut self, ids: Vec<WidgetId>) {
        self.child_ids = ids;
    }

    fn take_visible_when(&mut self) -> Option<Reactive<bool>> {
        self.visible_when_state.take()
    }

    fn take_enabled_when(&mut self) -> Option<Reactive<bool>> {
        self.enabled_when_state.take()
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
    fn children_get_intrinsic_widths() {
        let mut tree = WidgetTree::new();
        let a = tree.add(FixedLeaf(60.0, 30.0));
        let b = tree.add(FixedLeaf(40.0, 20.0));
        let _stack = tree.add(HStack::new().add_child(a).add_child(b));
        tree.layout(SizeProposal::exact(300.0, 50.0));

        assert!((tree.bounds(a).width - 60.0).abs() < 0.01);
        assert!((tree.bounds(b).width - 40.0).abs() < 0.01);
        assert!((tree.bounds(b).x - 60.0).abs() < 0.01);
    }

    #[test]
    fn spacing_between_children() {
        let mut tree = WidgetTree::new();
        let a = tree.add(FixedLeaf(50.0, 30.0));
        let b = tree.add(FixedLeaf(50.0, 30.0));
        let _stack = tree.add(HStack::new().spacing(10.0).add_child(a).add_child(b));
        tree.layout(SizeProposal::exact(300.0, 50.0));

        assert!((tree.bounds(a).x - 0.0).abs() < 0.01);
        assert!((tree.bounds(b).x - 60.0).abs() < 0.01); // 50 + 10
    }

    #[test]
    fn cross_axis_center_alignment() {
        let mut tree = WidgetTree::new();
        let a = tree.add(FixedLeaf(50.0, 20.0));
        let _stack = tree.add(HStack::new().add_child(a)); // default: VAlignment::Center
        tree.layout(SizeProposal::exact(200.0, 60.0));

        // 20px child centered in 60px height: y = (60-20)/2 = 20
        assert!((tree.bounds(a).y - 20.0).abs() < 0.01);
    }

    #[test]
    fn cross_axis_top_alignment() {
        let mut tree = WidgetTree::new();
        let a = tree.add(FixedLeaf(50.0, 20.0));
        let _stack = tree.add(HStack::new().alignment(VAlignment::Top).add_child(a));
        tree.layout(SizeProposal::exact(200.0, 60.0));

        assert!((tree.bounds(a).y - 0.0).abs() < 0.01);
    }

    #[test]
    fn cross_axis_bottom_alignment() {
        let mut tree = WidgetTree::new();
        let a = tree.add(FixedLeaf(50.0, 20.0));
        let _stack = tree.add(HStack::new().alignment(VAlignment::Bottom).add_child(a));
        tree.layout(SizeProposal::exact(200.0, 60.0));

        assert!((tree.bounds(a).y - 40.0).abs() < 0.01); // 60 - 20
    }

    #[test]
    fn per_child_alignment_override() {
        let mut tree = WidgetTree::new();
        let a = tree.add(FixedLeaf(50.0, 20.0));
        let b = tree.add(FixedLeaf(50.0, 20.0));
        // Container default: Top, but b overrides to Bottom
        let _stack = tree.add(
            HStack::new()
                .alignment(VAlignment::Top)
                .add_child(a)
                .add_child(b),
        );
        tree.set_alignment(
            b,
            fern_tokens::Alignment {
                horizontal: fern_tokens::HAlignment::Center,
                vertical: fern_tokens::VAlignment::Bottom,
            },
        );
        tree.layout(SizeProposal::exact(200.0, 60.0));

        assert!((tree.bounds(a).y - 0.0).abs() < 0.01); // Top
        assert!((tree.bounds(b).y - 40.0).abs() < 0.01); // Bottom override
    }

    #[test]
    fn intrinsic_size_sums_children() {
        let stack = HStack::new().spacing(5.0);
        // Without arena, size_that_fits falls back to proposal
        let theme = fern_tokens::Theme::light_default();
        let ctx = LayoutContext::for_testing(&theme);
        let size = stack.size_that_fits(SizeProposal::exact(100.0, 50.0), &ctx);
        // No children queryable without arena, so returns proposal
        assert_eq!(size.width, 100.0);
        assert_eq!(size.height, 50.0);
    }

    #[test]
    fn empty_hstack() {
        let mut tree = WidgetTree::new();
        let _stack = tree.add(HStack::new());
        tree.layout(SizeProposal::exact(200.0, 50.0));
        // No crash, no children to place
    }

    #[test]
    fn rtl_reverses_child_order() {
        let mut tree = WidgetTree::new();
        tree.set_layout_direction(fern_core::environment::LayoutDirection::RightToLeft);
        let a = tree.add(FixedLeaf(60.0, 30.0));
        let b = tree.add(FixedLeaf(40.0, 30.0));
        let _stack = tree.add(HStack::new().add_child(a).add_child(b));
        tree.layout(SizeProposal::exact(300.0, 50.0));

        // In RTL, first child (a) is placed on the right, second (b) on the left.
        // HStack without spacers sizes to content: 60+40 = 100px wide.
        let ab = tree.bounds(a);
        let bb = tree.bounds(b);
        assert!(ab.x > bb.x,
            "a.x={} should be > b.x={} in RTL", ab.x, bb.x);
        // a (60px) at right edge of 100px HStack
        assert!((ab.x - 40.0).abs() < 0.01, "a.x={}", ab.x);
        // b (40px) to the left of a
        assert!((bb.x - 0.0).abs() < 0.01, "b.x={}", bb.x);
    }
}
