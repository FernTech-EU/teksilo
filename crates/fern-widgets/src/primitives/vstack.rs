use fern_canvas::{Point, Rect, Size, SizeProposal};
use fern_core::accessibility::AccessNodeBuilder;
use fern_core::signal::Prop;
use fern_core::widget::{LayoutContext, PaintContext, PendingChild, Widget, WidgetPlacement};
use fern_core::widget_id::WidgetId;
use fern_tokens::HAlignment;

/// Vertical layout container that distributes children top-to-bottom
/// based on their intrinsic sizes. Cross-axis alignment is controlled
/// by `HAlignment` (default: `Leading`).
#[derive(Debug)]
pub struct VStack {
    child_ids: Vec<WidgetId>,
    pending: Vec<PendingChild>,
    spacing: Prop<f32>,
    alignment: HAlignment,
}

impl VStack {
    pub fn new() -> Self {
        Self {
            child_ids: Vec::new(),
            pending: Vec::new(),
            spacing: Prop::Static(0.0),
            alignment: HAlignment::Leading,
        }
    }

    /// Set inter-child spacing. Accepts a static `f32` or a reactive
    /// `Signal<f32>`.
    pub fn spacing(mut self, spacing: impl Into<Prop<f32>>) -> Self {
        self.spacing = spacing.into();
        self
    }

    pub fn alignment(mut self, alignment: HAlignment) -> Self {
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

        // Query each child's intrinsic size: height=None (ideal), width from
        // proposal. We ask every child, *including spacers*: a plain `Spacer`
        // reports `min_length` (0) at an unspecified height and so contributes
        // nothing to the total, but `Expand::fills_stack` wrapping a content
        // child reports that child's natural size — which must be included
        // or the stack under-reports its own intrinsic height and the
        // content spills over siblings.
        let child_proposal = SizeProposal {
            width: proposal.width,
            height: None,
        };

        let mut total_height: f32 = 0.0;
        let mut max_width: f32 = 0.0;

        for &child_id in &self.child_ids {
            if let Some(child_size) = ctx.child_size(child_id, child_proposal) {
                total_height += child_size.height;
                max_width = max_width.max(child_size.width);
            }
        }

        let n = self.child_ids.len();
        let spacing = self.spacing.get();
        let total_spacing = spacing * (n as f32 - 1.0).max(0.0);
        total_height += total_spacing;

        let width = proposal.width.unwrap_or(max_width);
        // Primary axis: if the container has spacers AND the parent gave us
        // a height, fill that height so spacers can stretch. Otherwise just
        // report `total_height` (which, with the loop above, now includes
        // content-carrying expands).
        let has_spacers = self.child_ids.iter().any(|&id| ctx.child_is_spacer(id));
        let height = if has_spacers {
            proposal.height.unwrap_or(total_height)
        } else {
            total_height
        };

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

        // Query each child's intrinsic size: offer the full container width
        // so children containing spacers (e.g. HStack) can expand properly.
        let intrinsic_proposal = SizeProposal {
            width: Some(bounds.width),
            height: None,
        };

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
        let spacing = self.spacing.get();
        let total_spacing = spacing * (n as f32 - 1.0).max(0.0);
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
            y += h + spacing;
        }
    }

    fn paint(&self, _bounds: Rect, _canvas: &mut fern_canvas::Canvas, _ctx: &PaintContext) {}

    fn accessibility(&self, builder: &mut AccessNodeBuilder) {
        builder.set_role(fern_core::accesskit::Role::GenericContainer);
    }

    fn children(&self) -> Vec<WidgetId> {
        self.child_ids.clone()
    }

    fn build(&mut self, ctx: &mut fern_core::build_context::BuildContext) -> Vec<WidgetId> {
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
        let self_id = ctx.self_id();
        let registry = ctx.binding_registry();
        self.spacing
            .register_if_bound(self_id, registry, fern_core::binding::BindingLevel::Relayout);
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
    fn nested_vstack_with_content_carrying_expand_reports_full_height() {
        // Regression for the TabWidget "tabs half-visible behind switcher"
        // bug. An inner VStack contains [leaf(32), Expand::fills_stack(leaf(200))].
        // Without the fix, VStack reports height=32 because spacers were
        // skipped in size_that_fits, so the outer VStack squashes the inner
        // stack into a 32-dp slot and the content spills over siblings.
        use crate::primitives::expand::Expand;

        let mut tree = WidgetTree::new();
        let tab_bar = tree.add(FixedLeaf(120.0, 32.0));
        let content = tree.add(FixedLeaf(120.0, 200.0));
        let filled = tree.add(Expand::vertical().fills_stack().child_id(content));
        let inner = tree.add(VStack::new().add_child(tab_bar).add_child(filled));

        // Outer VStack with another sibling underneath. Height is
        // unconstrained so the outer has to fall back to intrinsic sizes.
        let sibling = tree.add(FixedLeaf(120.0, 40.0));
        let outer = tree.add(VStack::new().add_child(inner).add_child(sibling));
        tree.layout(SizeProposal {
            width: Some(400.0),
            height: None,
        });

        // Inner stack should report 32 + 200 = 232, not 32.
        let inner_bounds = tree.bounds(inner);
        assert!(
            (inner_bounds.height - 232.0).abs() < 0.01,
            "inner VStack height should include the content-carrying Expand, got {}",
            inner_bounds.height,
        );

        // Sibling must sit BELOW the inner stack's full height, not overlap
        // it.
        let sibling_bounds = tree.bounds(sibling);
        assert!(
            sibling_bounds.y >= inner_bounds.bottom() - 0.01,
            "sibling should be placed below the inner stack; \
             inner bottom {}, sibling y {}",
            inner_bounds.bottom(),
            sibling_bounds.y,
        );

        // And the tab_bar should live at the TOP of the inner stack, with
        // the content filling the remaining ~200 dp below it.
        assert!((tree.bounds(tab_bar).y - inner_bounds.y).abs() < 0.01);
        let filled_bounds = tree.bounds(filled);
        assert!(filled_bounds.y >= inner_bounds.y + 32.0 - 0.01);

        // Outer container also behaves: its full height is 232 + 40 = 272.
        let outer_bounds = tree.bounds(outer);
        assert!(
            (outer_bounds.height - 272.0).abs() < 0.01,
            "outer VStack height got {}, expected 272",
            outer_bounds.height,
        );
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

    // --- Inline builder API tests ---

    #[test]
    fn child_inline_resolves_layout() {
        let mut tree = WidgetTree::new();
        let stack = tree.add(
            VStack::new()
                .child(FixedLeaf(80.0, 30.0))
                .child(FixedLeaf(60.0, 50.0)),
        );
        tree.layout(SizeProposal::exact(200.0, 300.0));

        let kids = tree.children(stack);
        assert_eq!(kids.len(), 2);
        assert!((tree.bounds(kids[0]).height - 30.0).abs() < 0.01);
        assert!((tree.bounds(kids[1]).height - 50.0).abs() < 0.01);
        assert!((tree.bounds(kids[1]).y - 30.0).abs() < 0.01);
    }

    #[test]
    fn mixed_add_child_and_inline_child() {
        let mut tree = WidgetTree::new();
        let pre = tree.add(FixedLeaf(80.0, 20.0));
        let stack = tree.add(VStack::new().add_child(pre).child(FixedLeaf(80.0, 40.0)));
        tree.layout(SizeProposal::exact(200.0, 300.0));

        let kids = tree.children(stack);
        assert_eq!(kids.len(), 2);
        assert_eq!(kids[0], pre);
        assert!((tree.bounds(kids[0]).height - 20.0).abs() < 0.01);
        assert!((tree.bounds(kids[1]).height - 40.0).abs() < 0.01);
        assert!((tree.bounds(kids[1]).y - 20.0).abs() < 0.01);
    }

    #[test]
    fn children_iterator() {
        let leaves: Vec<FixedLeaf> = vec![
            FixedLeaf(80.0, 10.0),
            FixedLeaf(80.0, 20.0),
            FixedLeaf(80.0, 30.0),
        ];
        let mut tree = WidgetTree::new();
        let stack = tree.add(VStack::new().children(leaves));
        tree.layout(SizeProposal::exact(200.0, 300.0));

        let kids = tree.children(stack);
        assert_eq!(kids.len(), 3);
        assert!((tree.bounds(kids[2]).y - 30.0).abs() < 0.01); // 10 + 20
    }

    #[test]
    fn child_opt_none_is_noop() {
        let mut tree = WidgetTree::new();
        let stack = tree.add(
            VStack::new()
                .child(FixedLeaf(80.0, 30.0))
                .child_opt(None::<FixedLeaf>)
                .child(FixedLeaf(80.0, 50.0)),
        );
        tree.layout(SizeProposal::exact(200.0, 300.0));

        let kids = tree.children(stack);
        assert_eq!(kids.len(), 2);
    }

    #[test]
    fn child_opt_some_adds_child() {
        let mut tree = WidgetTree::new();
        let stack = tree.add(VStack::new().child_opt(Some(FixedLeaf(80.0, 25.0))));
        tree.layout(SizeProposal::exact(200.0, 300.0));

        let kids = tree.children(stack);
        assert_eq!(kids.len(), 1);
        assert!((tree.bounds(kids[0]).height - 25.0).abs() < 0.01);
    }

    #[test]
    fn nested_inline_children() {
        use crate::primitives::hstack::HStack;

        let mut tree = WidgetTree::new();
        let outer = tree.add(
            VStack::new()
                .child(
                    HStack::new()
                        .child(FixedLeaf(40.0, 30.0))
                        .child(FixedLeaf(50.0, 30.0)),
                )
                .child(FixedLeaf(80.0, 20.0)),
        );
        tree.layout(SizeProposal::exact(200.0, 300.0));

        let outer_kids = tree.children(outer);
        assert_eq!(outer_kids.len(), 2);
        // The HStack should have 2 children
        let hstack_kids = tree.children(outer_kids[0]);
        assert_eq!(hstack_kids.len(), 2);
        // Second VStack child starts after HStack height (30)
        assert!((tree.bounds(outer_kids[1]).y - 30.0).abs() < 0.01);
    }

    #[test]
    fn single_child_wrapper_inline() {
        use crate::primitives::padding::Padding;

        let mut tree = WidgetTree::new();
        let stack =
            tree.add(VStack::new().child(Padding::uniform(10.0).child(FixedLeaf(80.0, 30.0))));
        tree.layout(SizeProposal::exact(200.0, 300.0));

        let kids = tree.children(stack);
        assert_eq!(kids.len(), 1);
        // Padding adds 10 on each side: 30 + 20 = 50
        assert!((tree.bounds(kids[0]).height - 50.0).abs() < 0.01);
    }

    #[test]
    fn dormant_child_does_not_take_layout_space() {
        let mut tree = WidgetTree::new();
        let a = tree.add(FixedLeaf(80.0, 30.0));
        let b = tree.add(FixedLeaf(80.0, 40.0));
        let c = tree.add(FixedLeaf(80.0, 50.0));
        let _stack = tree.add(
            VStack::new()
                .spacing(10.0)
                .add_child(a)
                .add_child(b)
                .add_child(c),
        );
        tree.layout(SizeProposal::exact(200.0, 300.0));

        // Before dormant: a(0..30), gap(10), b(40..80), gap(10), c(90..140)
        assert!((tree.bounds(c).y - 90.0).abs() < 0.01);

        // Make middle child dormant
        tree.set_dormant(b);
        tree.layout(SizeProposal::exact(200.0, 300.0));

        // After dormant: a(0..30), gap(10), c(40..90) — b's space is reclaimed
        assert!((tree.bounds(c).y - 40.0).abs() < 0.01);
    }

    #[test]
    fn dormant_child_via_visible_when_does_not_take_layout_space() {
        use fern_core::signal::Signal;

        let show_b = Signal::new(true);
        let mut tree = WidgetTree::new();
        let a = tree.add(FixedLeaf(80.0, 30.0));
        let b = tree.add(FixedLeaf(80.0, 40.0));
        tree.visible_when(b, show_b.clone());
        let c = tree.add(FixedLeaf(80.0, 50.0));
        let _stack = tree.add(
            VStack::new()
                .spacing(10.0)
                .add_child(a)
                .add_child(b)
                .add_child(c),
        );
        tree.layout(SizeProposal::exact(200.0, 300.0));

        // All visible: a(0..30), gap(10), b(40..80), gap(10), c(90..140)
        assert!((tree.bounds(c).y - 90.0).abs() < 0.01);

        // Hide b via state
        show_b.set(false);
        tree.layout(SizeProposal::exact(200.0, 300.0));

        // b is dormant: a(0..30), gap(10), c(40..90)
        assert!((tree.bounds(c).y - 40.0).abs() < 0.01);

        // Show b again
        show_b.set(true);
        tree.layout(SizeProposal::exact(200.0, 300.0));

        // Back to original layout
        assert!((tree.bounds(c).y - 90.0).abs() < 0.01);
    }
}
