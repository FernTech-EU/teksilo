use fern_canvas::{Point, Rect, Size, SizeProposal};
use fern_core::state::State;
use fern_core::widget::{IntoWidgetTree, LayoutContext, PaintContext, PendingChild, Widget, WidgetPlacement};
use fern_core::widget_id::WidgetId;
use fern_tokens::HAlignment;

/// Vertical layout container that distributes children top-to-bottom
/// based on their intrinsic sizes. Cross-axis alignment is controlled
/// by `HAlignment` (default: `Leading`).
#[derive(Debug)]
pub struct VStack {
    child_ids: Vec<WidgetId>,
    pending: Vec<PendingChild>,
    spacing: f32,
    alignment: HAlignment,
    visible_when_state: Option<State<bool>>,
    enabled_when_state: Option<State<bool>>,
}

impl VStack {
    pub fn new() -> Self {
        Self {
            child_ids: Vec::new(),
            pending: Vec::new(),
            spacing: 0.0,
            alignment: HAlignment::Leading,
            visible_when_state: None,
            enabled_when_state: None,
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
    pub fn visible_when(mut self, state: State<bool>) -> Self {
        self.visible_when_state = Some(state);
        self
    }

    /// Bind enabled state to a boolean state.
    pub fn enabled_when(mut self, state: State<bool>) -> Self {
        self.enabled_when_state = Some(state);
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

    fn take_pending_children(&mut self) -> Vec<PendingChild> {
        std::mem::take(&mut self.pending)
    }

    fn set_resolved_children(&mut self, ids: Vec<WidgetId>) {
        self.child_ids = ids;
    }

    fn take_visible_when(&mut self) -> Option<State<bool>> {
        self.visible_when_state.take()
    }

    fn take_enabled_when(&mut self) -> Option<State<bool>> {
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
        let stack = tree.add(
            VStack::new()
                .add_child(pre)
                .child(FixedLeaf(80.0, 40.0)),
        );
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
        let stack = tree.add(
            VStack::new()
                .child_opt(Some(FixedLeaf(80.0, 25.0))),
        );
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
        let stack = tree.add(
            VStack::new()
                .child(
                    Padding::uniform(10.0)
                        .child(FixedLeaf(80.0, 30.0)),
                ),
        );
        tree.layout(SizeProposal::exact(200.0, 300.0));

        let kids = tree.children(stack);
        assert_eq!(kids.len(), 1);
        // Padding adds 10 on each side: 30 + 20 = 50
        assert!((tree.bounds(kids[0]).height - 50.0).abs() < 0.01);
    }

    // --- visible_when / enabled_when builder tests ---

    #[test]
    fn visible_when_builder_registers_binding() {
        use fern_core::state::State;

        let show = State::new(true);
        let mut tree = WidgetTree::new();
        let panel_id = tree.add(
            VStack::new()
                .child(FixedLeaf(80.0, 30.0))
                .visible_when(show.clone()),
        );

        tree.layout(SizeProposal::exact(200.0, 300.0));
        // Initially visible
        assert!(tree.is_visible(panel_id));

        // Set state to false → widget becomes dormant after next layout
        show.set(false);
        tree.layout(SizeProposal::exact(200.0, 300.0));
        assert!(!tree.is_visible(panel_id));

        // Set state back to true → widget is active again
        show.set(true);
        tree.layout(SizeProposal::exact(200.0, 300.0));
        assert!(tree.is_visible(panel_id));
    }

    #[test]
    fn enabled_when_builder_registers_binding() {
        use fern_core::state::State;

        let can_act = State::new(true);
        let mut tree = WidgetTree::new();
        let stack_id = tree.add(
            VStack::new()
                .child(FixedLeaf(80.0, 30.0))
                .enabled_when(can_act.clone()),
        );

        assert!(tree.is_enabled(stack_id));

        can_act.set(false);
        tree.layout(SizeProposal::exact(200.0, 300.0));
        assert!(!tree.is_enabled(stack_id));
    }
}
