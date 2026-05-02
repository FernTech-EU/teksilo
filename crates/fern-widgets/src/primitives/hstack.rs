use fern_canvas::{Point, Rect, Size, SizeProposal};
use fern_core::accessibility::AccessNodeBuilder;
use fern_core::signal::Prop;
use fern_core::widget::{LayoutContext, PaintContext, PendingChild, Widget, WidgetPlacement};
use fern_core::widget_id::WidgetId;
use fern_tokens::VAlignment;

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
    ) -> fern_core::widget::LayoutResponse {
        if self.child_ids.is_empty() {
            return proposal.resolve(0.0, 0.0).into();
        }

        // SwiftUI-shape negotiation: ask each child for its full
        // LayoutResponse (wanted size + flex weight) in a single call.
        let child_proposal = SizeProposal {
            width: None,
            height: proposal.height,
        };

        let mut total_wanted: f32 = 0.0;
        let mut max_height: f32 = 0.0;
        let mut total_flex: f32 = 0.0;
        for &child_id in &self.child_ids {
            if let Some(r) = ctx.child_layout_response(child_id, child_proposal) {
                total_wanted += r.size.width;
                max_height = max_height.max(r.size.height);
                total_flex += r.flex;
            }
        }

        let n = self.child_ids.len();
        let spacing = self.spacing.get();
        let total_spacing = spacing * (n as f32 - 1.0).max(0.0);
        let content_width = total_wanted + total_spacing;

        // If any child is flex (wants slack), greedily claim the parent's
        // offered width so slack exists to distribute. Otherwise honestly
        // report content_width.
        let width = if total_flex > 0.0 {
            proposal.width.unwrap_or(content_width)
        } else {
            content_width
        };
        let height = proposal.height.unwrap_or(max_height);

        Size::new(width, height).into()
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

        // Single-query negotiation: ask each child for size + flex.
        let wanted_proposal = SizeProposal {
            width: None,
            height: Some(bounds.height),
        };

        let mut wanted_widths: Vec<f32> = Vec::with_capacity(n);
        let mut wanted_heights: Vec<f32> = Vec::with_capacity(n);
        let mut flex_factors: Vec<f32> = Vec::with_capacity(n);
        let mut total_wanted: f32 = 0.0;
        let mut total_flex: f32 = 0.0;

        for child in children.iter() {
            let r = ctx
                .child_layout_response(child.id, wanted_proposal)
                .unwrap_or(fern_core::widget::LayoutResponse::ZERO);
            wanted_widths.push(r.size.width);
            wanted_heights.push(r.size.height);
            flex_factors.push(r.flex);
            total_wanted += r.size.width;
            total_flex += r.flex;
        }

        // Slack is leftover space after honoring every child's wanted size
        // and inter-child spacing. Distributed proportionally to flex.
        let spacing = self.spacing.get();
        let total_spacing = spacing * (n as f32 - 1.0).max(0.0);
        let slack = (bounds.width - total_wanted - total_spacing).max(0.0);

        let mut final_widths: Vec<f32> = Vec::with_capacity(n);
        for i in 0..n {
            let bonus = if total_flex > 0.0 {
                (flex_factors[i] / total_flex) * slack
            } else {
                0.0
            };
            final_widths.push(wanted_widths[i] + bonus);
        }

        // Place children with alignment on cross axis.
        // In RTL mode, children are placed right-to-left.
        let rtl = ctx.is_rtl();
        if rtl {
            let mut x = bounds.right();
            for (i, child) in children.iter_mut().enumerate() {
                let w = final_widths[i];
                let h = wanted_heights[i];
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
                let w = final_widths[i];
                let h = wanted_heights[i];
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
        // Register the spacing prop for dirty-tracking so theme-driven
        // signals trigger a relayout when they change.
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
        fn layout_response(&self, _proposal: SizeProposal, _ctx: &LayoutContext) -> fern_core::widget::LayoutResponse {
            Size::new(self.0, self.1).into()
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
        let _stack = tree.add(
            HStack::new()
                .add_child(fixed)
                .add_child(a)
                .add_child(b),
        );
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
        tree.set_layout_direction(fern_core::environment::LayoutDirection::RightToLeft);
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
