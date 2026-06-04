use bastyde_canvas::{Point, Rect, Size, SizeProposal};
use bastyde_core::accessibility::AccessNodeBuilder;
use bastyde_core::signal::Prop;
use bastyde_core::widget::{LayoutContext, PaintContext, PendingChild, Widget, WidgetPlacement};
use bastyde_core::widget_id::WidgetId;
use bastyde_tokens::VAlignment;

use crate::primitives::linear_layout::{self, Axis};

/// Horizontal layout container that distributes children left-to-right
/// based on their intrinsic sizes. Cross-axis alignment is controlled
/// by `VAlignment` (default: `Center`).
#[derive(Debug)]
pub struct HStack {
    child_ids: Vec<WidgetId>,
    pending: Vec<PendingChild>,
    spacing: Prop<f32>,
    alignment: VAlignment,
}

impl HStack {
    pub fn new() -> Self {
        Self {
            child_ids: Vec::new(),
            pending: Vec::new(),
            spacing: Prop::Static(0.0),
            alignment: VAlignment::Center,
        }
    }

    /// Set inter-child spacing. Accepts a static `f32` or a reactive
    /// `Signal<f32>` — use a signal derived from
    /// `ctx.theme_signal()` to track theme-driven spacing changes.
    pub fn spacing(mut self, spacing: impl Into<Prop<f32>>) -> Self {
        self.spacing = spacing.into();
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

impl Default for HStack {
    fn default() -> Self {
        Self::new()
    }
}

impl Widget for HStack {
    fn layout_response(
        &self,
        proposal: SizeProposal,
        ctx: &LayoutContext,
    ) -> bastyde_core::widget::LayoutResponse {
        if self.child_ids.is_empty() {
            return proposal.resolve(0.0, 0.0).into();
        }
        // Main-then-cross negotiation along the horizontal axis: grow on
        // surplus (flex), shrink on a deficit (shrink/min), and measure each
        // child's height at its final width (height-for-width). See
        // [`linear_layout`].
        let neg = linear_layout::negotiate(
            &self.child_ids,
            ctx,
            proposal.width,
            proposal.height,
            self.spacing.get(),
            Axis::Horizontal,
        );
        linear_layout::response(&neg)
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

        let ids: Vec<WidgetId> = children.iter().map(|c| c.id).collect();
        let neg = linear_layout::negotiate(
            &ids,
            ctx,
            Some(bounds.width),
            Some(bounds.height),
            self.spacing.get(),
            Axis::Horizontal,
        );
        let widths = &neg.children.main;
        let heights = &neg.children.cross;

        // Place children along the main axis with cross-axis (vertical)
        // alignment. In RTL mode, children are placed right-to-left.
        let spacing = self.spacing.get();
        let rtl = ctx.is_rtl();
        if rtl {
            let mut x = bounds.right();
            for (i, child) in children.iter_mut().enumerate() {
                let w = widths[i];
                let h = heights[i];
                let valign = ctx
                    .child_alignment(child.id)
                    .map(|a| a.vertical)
                    .unwrap_or(self.alignment);
                let y_offset = valign.resolve(h, bounds.height);

                x -= w;
                child.origin = Point::new(x, bounds.y + y_offset);
                child.size = Size::new(w, h);
                x -= spacing;
            }
        } else {
            let mut x = bounds.x;
            for (i, child) in children.iter_mut().enumerate() {
                let w = widths[i];
                let h = heights[i];
                let valign = ctx
                    .child_alignment(child.id)
                    .map(|a| a.vertical)
                    .unwrap_or(self.alignment);
                let y_offset = valign.resolve(h, bounds.height);

                child.origin = Point::new(x, bounds.y + y_offset);
                child.size = Size::new(w, h);
                x += w + spacing;
            }
        }
    }

    fn paint(&self, _bounds: Rect, _canvas: &mut bastyde_canvas::Canvas, _ctx: &PaintContext) {}

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
        // Register the spacing prop for dirty-tracking so theme-driven
        // signals trigger a relayout when they change.
        let self_id = ctx.self_id();
        let registry = ctx.binding_registry();
        self.spacing.register_if_bound(
            self_id,
            registry,
            bastyde_core::binding::BindingLevel::Relayout,
        );
        self.child_ids.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bastyde_core::widget_tree::WidgetTree;

    /// A leaf that always reports a fixed intrinsic size.
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

    use bastyde_core::widget::LayoutResponse;

    /// A leaf with an explicit compression floor and shrink weight.
    #[derive(Debug)]
    struct ShrinkLeaf {
        wanted: f32,
        min: f32,
        shrink: f32,
        height: f32,
    }
    impl Widget for ShrinkLeaf {
        fn layout_response(&self, _p: SizeProposal, _ctx: &LayoutContext) -> LayoutResponse {
            LayoutResponse::shrinkable(
                Size::new(self.wanted, self.height),
                Size::new(self.min, self.height),
                self.shrink,
            )
        }
    }

    /// A height-for-width leaf: fixed "area", so its height is `area / width`
    /// at whatever width it is finally given (narrower → taller). Shrinkable
    /// down to `min_w`.
    #[derive(Debug)]
    struct AreaLeaf {
        area: f32,
        wanted_w: f32,
        min_w: f32,
    }
    impl Widget for AreaLeaf {
        fn layout_response(&self, p: SizeProposal, _ctx: &LayoutContext) -> LayoutResponse {
            let w = p.width.unwrap_or(self.wanted_w);
            let h = self.area / w.max(1.0);
            LayoutResponse::shrinkable(Size::new(w, h), Size::new(self.min_w, h), 1.0)
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

    // ── Over-constraint: shrink (Part B) ────────────────────────────────────

    #[test]
    fn shrink_distributes_deficit_by_weight() {
        // Two equal shrinkables, 60px deficit → 30px each.
        let mut tree = WidgetTree::new();
        let a = tree.add(ShrinkLeaf {
            wanted: 80.0,
            min: 20.0,
            shrink: 1.0,
            height: 20.0,
        });
        let b = tree.add(ShrinkLeaf {
            wanted: 80.0,
            min: 20.0,
            shrink: 1.0,
            height: 20.0,
        });
        let _stack = tree.add(HStack::new().add_child(a).add_child(b));
        tree.layout(SizeProposal::exact(100.0, 40.0));
        assert!(
            (tree.bounds(a).width - 50.0).abs() < 0.01,
            "a={}",
            tree.bounds(a).width
        );
        assert!(
            (tree.bounds(b).width - 50.0).abs() < 0.01,
            "b={}",
            tree.bounds(b).width
        );
    }

    #[test]
    fn shrink_priority_leaves_rigid_sibling_untouched() {
        // Shrinkable label absorbs the whole deficit; rigid icon keeps its size.
        let mut tree = WidgetTree::new();
        let label = tree.add(ShrinkLeaf {
            wanted: 80.0,
            min: 10.0,
            shrink: 1.0,
            height: 20.0,
        });
        let icon = tree.add(FixedLeaf(40.0, 20.0)); // rigid: shrink == 0
        let _stack = tree.add(HStack::new().add_child(label).add_child(icon));
        tree.layout(SizeProposal::exact(100.0, 40.0)); // deficit = 120 - 100 = 20
        assert!(
            (tree.bounds(label).width - 60.0).abs() < 0.01,
            "label={}",
            tree.bounds(label).width
        );
        assert!(
            (tree.bounds(icon).width - 40.0).abs() < 0.01,
            "icon={}",
            tree.bounds(icon).width
        );
    }

    #[test]
    fn shrink_clamps_at_min_with_residual_overflow() {
        // Deficit exceeds available shrink room: clamp at min, do not go below.
        let mut tree = WidgetTree::new();
        let a = tree.add(ShrinkLeaf {
            wanted: 80.0,
            min: 50.0,
            shrink: 1.0,
            height: 20.0,
        });
        let _stack = tree.add(HStack::new().add_child(a));
        tree.layout(SizeProposal::exact(30.0, 40.0)); // wants to shrink to 30, floored at 50
        assert!(
            (tree.bounds(a).width - 50.0).abs() < 0.01,
            "a={}",
            tree.bounds(a).width
        );
    }

    #[test]
    fn no_shrink_weight_overflows_unchanged() {
        // A rigid child still overflows (no silent shrink without opt-in).
        let mut tree = WidgetTree::new();
        let a = tree.add(FixedLeaf(80.0, 20.0));
        let _stack = tree.add(HStack::new().add_child(a));
        tree.layout(SizeProposal::exact(50.0, 40.0));
        assert!(
            (tree.bounds(a).width - 80.0).abs() < 0.01,
            "a={}",
            tree.bounds(a).width
        );
    }

    // ── Height-for-width (Part B) ───────────────────────────────────────────

    #[test]
    fn height_for_width_grows_cross_axis_on_shrink() {
        // AreaLeaf: area 4000, natural width 200 → height 20. Constrained to
        // width 50, it shrinks to 50 and its height becomes 4000/50 = 80; the
        // HStack's reported cross size must follow.
        let mut tree = WidgetTree::new();
        let leaf = tree.add(AreaLeaf {
            area: 4000.0,
            wanted_w: 200.0,
            min_w: 10.0,
        });
        let stack = tree.add(HStack::new().add_child(leaf));
        tree.layout(SizeProposal {
            width: Some(50.0),
            height: None, // ask the stack for its intrinsic height
        });
        assert!(
            (tree.bounds(leaf).width - 50.0).abs() < 0.01,
            "leaf w={}",
            tree.bounds(leaf).width
        );
        assert!(
            (tree.bounds(leaf).height - 80.0).abs() < 0.5,
            "leaf h={}",
            tree.bounds(leaf).height
        );
        assert!(
            (tree.bounds(stack).height - 80.0).abs() < 0.5,
            "stack height should follow height-for-width, got {}",
            tree.bounds(stack).height
        );
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
            bastyde_tokens::Alignment {
                horizontal: bastyde_tokens::HAlignment::Center,
                vertical: bastyde_tokens::VAlignment::Bottom,
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
        let theme = bastyde_core::presets::intui::light();
        let ctx = LayoutContext::for_testing(&theme);
        let size = stack
            .layout_response(SizeProposal::exact(100.0, 50.0), &ctx)
            .size;
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
    fn flex_distributes_proportionally() {
        // [Expand::flex(1), Expand::flex(2)] in 300px → 100, 200.
        use crate::primitives::expand::Expand;
        let mut tree = WidgetTree::new();
        let a = tree.add(Expand::new().flex(1.0));
        let b = tree.add(Expand::new().flex(2.0));
        let _stack = tree.add(HStack::new().add_child(a).add_child(b));
        tree.layout(SizeProposal::exact(300.0, 50.0));

        assert!(
            (tree.bounds(a).width - 100.0).abs() < 0.01,
            "a.width={}",
            tree.bounds(a).width
        );
        assert!(
            (tree.bounds(b).width - 200.0).abs() < 0.01,
            "b.width={}",
            tree.bounds(b).width
        );
    }

    #[test]
    fn flex_with_rigid_floor() {
        // [Fixed(100), Expand::flex(1), Expand::flex(2)] in 400 → 100, 100, 200.
        use crate::primitives::expand::Expand;
        let mut tree = WidgetTree::new();
        let fixed = tree.add(FixedLeaf(100.0, 30.0));
        let a = tree.add(Expand::new().flex(1.0));
        let b = tree.add(Expand::new().flex(2.0));
        let _stack = tree.add(HStack::new().add_child(fixed).add_child(a).add_child(b));
        tree.layout(SizeProposal::exact(400.0, 50.0));

        assert!((tree.bounds(fixed).width - 100.0).abs() < 0.01);
        assert!((tree.bounds(a).width - 100.0).abs() < 0.01);
        assert!((tree.bounds(b).width - 200.0).abs() < 0.01);
    }

    #[test]
    fn spacer_min_length_is_floor_plus_share() {
        // [Spacer::min(20), Spacer::min(20)] in 100 → 50, 50
        // (each gets 20 floor + 30 share).
        use crate::primitives::spacer::Spacer;
        let mut tree = WidgetTree::new();
        let a = tree.add(Spacer::new().min_length(20.0));
        let b = tree.add(Spacer::new().min_length(20.0));
        let _stack = tree.add(HStack::new().add_child(a).add_child(b));
        tree.layout(SizeProposal::exact(100.0, 50.0));

        assert!((tree.bounds(a).width - 50.0).abs() < 0.01);
        assert!((tree.bounds(b).width - 50.0).abs() < 0.01);
    }

    #[test]
    fn expand_default_is_flex_one_in_hstack() {
        // `Expand::new()` (no .flex(n)) inside HStack claims all leftover.
        // The footgun fix: previously `Expand::new()` collapsed without
        // `.fills_stack()`.
        use crate::primitives::expand::Expand;
        let mut tree = WidgetTree::new();
        let fixed = tree.add(FixedLeaf(80.0, 30.0));
        let expand = tree.add(Expand::new());
        let _stack = tree.add(HStack::new().add_child(fixed).add_child(expand));
        tree.layout(SizeProposal::exact(200.0, 50.0));

        assert!((tree.bounds(fixed).width - 80.0).abs() < 0.01);
        assert!((tree.bounds(expand).width - 120.0).abs() < 0.01);
    }

    #[test]
    fn respect_intrinsic_uses_child_natural_as_floor() {
        // With respect_intrinsic, Expand::flex(1) wrapping a 60px child
        // contributes 60 to the rigid pool, then gets the remaining slack.
        // [Expand::flex(1).respect_intrinsic(child=60), Fixed(100)] in 300:
        //   - Expand wants 60 (auto-basis), flex=1
        //   - Fixed wants 100, no flex
        //   - slack = 300 - 60 - 100 = 140, all to Expand
        //   - Final: Expand = 60 + 140 = 200, Fixed = 100
        use crate::primitives::expand::Expand;
        let mut tree = WidgetTree::new();
        let inner = tree.add(FixedLeaf(60.0, 20.0));
        let expand = tree.add(Expand::new().respect_intrinsic().child_id(inner));
        let fixed = tree.add(FixedLeaf(100.0, 30.0));
        let _stack = tree.add(HStack::new().add_child(expand).add_child(fixed));
        tree.layout(SizeProposal::exact(300.0, 50.0));

        assert!((tree.bounds(expand).width - 200.0).abs() < 0.01);
        assert!((tree.bounds(fixed).width - 100.0).abs() < 0.01);
    }

    #[test]
    fn rtl_reverses_child_order() {
        let mut tree = WidgetTree::new();
        tree.set_layout_direction(bastyde_core::environment::LayoutDirection::RightToLeft);
        let a = tree.add(FixedLeaf(60.0, 30.0));
        let b = tree.add(FixedLeaf(40.0, 30.0));
        let _stack = tree.add(HStack::new().add_child(a).add_child(b));
        tree.layout(SizeProposal {
            width: None,
            height: Some(50.0),
        });

        // In RTL, first child (a) is placed on the right, second (b) on the left.
        // HStack without spacers sizes to content: 60+40 = 100px wide.
        let ab = tree.bounds(a);
        let bb = tree.bounds(b);
        assert!(ab.x > bb.x, "a.x={} should be > b.x={} in RTL", ab.x, bb.x);
        // a (60px) at right edge of 100px HStack
        assert!((ab.x - 40.0).abs() < 0.01, "a.x={}", ab.x);
        // b (40px) to the left of a
        assert!((bb.x - 0.0).abs() < 0.01, "b.x={}", bb.x);
    }
}
