//! Wrap — a horizontal flow layout that wraps children to the next line
//! when they exceed the available width.

use fern_canvas::{Point, Rect, Size, SizeProposal};
use fern_core::accessibility::AccessNodeBuilder;
use fern_core::signal::Prop;
use fern_core::widget::{LayoutContext, PaintContext, PendingChild, Widget, WidgetPlacement};
use fern_core::widget_id::WidgetId;

/// A horizontal flow layout that wraps children to the next line.
#[derive(Debug)]
pub struct Wrap {
    spacing: Prop<f32>,
    line_spacing: Prop<f32>,
    child_ids: Vec<WidgetId>,
    pending: Vec<PendingChild>,
}

impl Wrap {
    pub fn new() -> Self {
        Self {
            spacing: Prop::Static(0.0),
            line_spacing: Prop::Static(0.0),
            child_ids: Vec::new(),
            pending: Vec::new(),
        }
    }

    /// Horizontal spacing between items on the same line. Accepts a static
    /// `f32` or a reactive `Signal<f32>`.
    pub fn spacing(mut self, spacing: impl Into<Prop<f32>>) -> Self {
        self.spacing = spacing.into();
        self
    }

    /// Vertical spacing between lines. Accepts a static `f32` or a
    /// reactive `Signal<f32>`.
    pub fn line_spacing(mut self, spacing: impl Into<Prop<f32>>) -> Self {
        self.line_spacing = spacing.into();
        self
    }

    pub fn add_child(mut self, id: WidgetId) -> Self {
        self.pending.push(PendingChild::Id(id));
        self
    }

    pub fn child(mut self, widget: impl Widget + 'static) -> Self {
        self.pending.push(PendingChild::Deferred(Box::new(widget)));
        self
    }

    pub fn children(mut self, iter: impl IntoIterator<Item = impl Widget + 'static>) -> Self {
        for widget in iter {
            self.pending.push(PendingChild::Deferred(Box::new(widget)));
        }
        self
    }

    pub fn child_opt(mut self, widget: Option<impl Widget + 'static>) -> Self {
        if let Some(w) = widget {
            self.pending.push(PendingChild::Deferred(Box::new(w)));
        }
        self
    }

    /// Compute the line layout: returns (sizes, line_breaks).
    /// line_breaks[i] = true means a new line starts before child i.
    fn compute_layout(
        &self,
        available_width: f32,
        children: &[WidgetId],
        ctx: &LayoutContext,
    ) -> (Vec<Size>, Vec<bool>) {
        let child_proposal = SizeProposal::unspecified();
        let mut sizes = Vec::with_capacity(children.len());
        let mut line_breaks = vec![false; children.len()];
        let mut x = 0.0_f32;
        let spacing = self.spacing.get();

        for (i, &child_id) in children.iter().enumerate() {
            let size = ctx
                .child_size(child_id, child_proposal)
                .unwrap_or(Size::ZERO);
            sizes.push(size);

            if i > 0 {
                let needed = x + spacing + size.width;
                if needed > available_width {
                    line_breaks[i] = true;
                    x = size.width;
                } else {
                    x += spacing + size.width;
                }
            } else {
                x = size.width;
            }
        }
        (sizes, line_breaks)
    }
}

impl Default for Wrap {
    fn default() -> Self {
        Self::new()
    }
}

impl Widget for Wrap {
    fn layout_response(&self, proposal: SizeProposal, ctx: &LayoutContext) -> fern_core::widget::LayoutResponse {
        if self.child_ids.is_empty() {
            return (proposal.resolve(0.0, 0.0)).into();
        }

        let available_width = proposal.width.unwrap_or(f32::MAX);
        let (sizes, line_breaks) = self.compute_layout(available_width, &self.child_ids, ctx);

        let spacing = self.spacing.get();
        let line_spacing = self.line_spacing.get();
        let mut max_line_width = 0.0_f32;
        let mut line_width = 0.0_f32;
        let mut line_height = 0.0_f32;
        let mut total_height = 0.0_f32;
        let mut line_count = 0;

        for (i, size) in sizes.iter().enumerate() {
            if line_breaks[i] || i == 0 {
                if i > 0 {
                    max_line_width = max_line_width.max(line_width);
                    total_height += line_height;
                    line_count += 1;
                }
                line_width = size.width;
                line_height = size.height;
            } else {
                line_width += spacing + size.width;
                line_height = line_height.max(size.height);
            }
        }
        // Last line
        max_line_width = max_line_width.max(line_width);
        total_height += line_height;
        line_count += 1;

        let total_line_gap = line_spacing * (line_count as f32 - 1.0).max(0.0);
        Size::new(max_line_width, total_height + total_line_gap).into()
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

        let active_ids: Vec<WidgetId> = children.iter().map(|c| c.id).collect();
        let (sizes, line_breaks) = self.compute_layout(bounds.width, &active_ids, ctx);
        let spacing = self.spacing.get();
        let line_spacing = self.line_spacing.get();
        let mut x = bounds.x;
        let mut y = bounds.y;
        let mut line_height = 0.0_f32;

        for (i, child) in children.iter_mut().enumerate() {
            if i >= sizes.len() {
                break;
            }
            if line_breaks[i] {
                y += line_height + line_spacing;
                x = bounds.x;
                line_height = 0.0;
            }

            child.origin = Point::new(x, y);
            child.size = sizes[i];
            line_height = line_height.max(sizes[i].height);

            x += sizes[i].width + spacing;
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
        self.line_spacing.register_if_bound(
            self_id,
            registry,
            fern_core::binding::BindingLevel::Relayout,
        );
        self.child_ids.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use fern_core::widget_tree::WidgetTree;

    #[derive(Debug)]
    struct FixedLeaf(f32, f32);
    impl Widget for FixedLeaf {
        fn layout_response(&self, _proposal: SizeProposal, _ctx: &LayoutContext) -> fern_core::widget::LayoutResponse {
            Size::new(self.0, self.1).into()
        }
    }

    #[test]
    fn single_line_no_wrap() {
        let mut tree = WidgetTree::new();
        let a = tree.add(FixedLeaf(40.0, 20.0));
        let b = tree.add(FixedLeaf(40.0, 20.0));
        let _wrap = tree.add(Wrap::new().spacing(10.0).add_child(a).add_child(b));
        tree.layout(SizeProposal::exact(200.0, 100.0));

        assert!((tree.bounds(a).y - 0.0).abs() < 0.01);
        assert!((tree.bounds(b).y - 0.0).abs() < 0.01);
        assert!((tree.bounds(b).x - 50.0).abs() < 0.01); // 40 + 10
    }

    #[test]
    fn wraps_to_next_line() {
        let mut tree = WidgetTree::new();
        let a = tree.add(FixedLeaf(80.0, 20.0));
        let b = tree.add(FixedLeaf(80.0, 20.0));
        let c = tree.add(FixedLeaf(80.0, 20.0));
        let _wrap = tree.add(
            Wrap::new()
                .spacing(10.0)
                .line_spacing(5.0)
                .add_child(a)
                .add_child(b)
                .add_child(c),
        );
        tree.layout(SizeProposal::exact(200.0, 200.0));

        // 80 + 10 + 80 = 170 fits in 200, so a and b on line 1
        assert!((tree.bounds(a).y - 0.0).abs() < 0.01);
        assert!((tree.bounds(b).y - 0.0).abs() < 0.01);
        // 170 + 10 + 80 = 260 > 200, so c wraps to line 2
        assert!((tree.bounds(c).y - 25.0).abs() < 0.01); // 20 + 5 line_spacing
        assert!((tree.bounds(c).x - 0.0).abs() < 0.01); // starts at beginning
    }

    #[test]
    fn intrinsic_height_accounts_for_wrapping() {
        let mut tree = WidgetTree::new();
        let a = tree.add(FixedLeaf(60.0, 20.0));
        let b = tree.add(FixedLeaf(60.0, 30.0));
        let c = tree.add(FixedLeaf(60.0, 20.0));
        let wrap = tree.add(
            Wrap::new()
                .spacing(10.0)
                .line_spacing(5.0)
                .add_child(a)
                .add_child(b)
                .add_child(c),
        );
        tree.layout(SizeProposal {
            width: Some(140.0),
            height: None,
        });

        // Line 1: a(60) + 10 + b(60) = 130 fits in 140
        // Line 2: c(60)
        // Height: max(20,30) + 5 + 20 = 55
        let wb = tree.bounds(wrap);
        assert!((wb.height - 55.0).abs() < 0.01);
    }

    #[test]
    fn empty_wrap() {
        let mut tree = WidgetTree::new();
        let _wrap = tree.add(Wrap::new());
        tree.layout(SizeProposal::exact(200.0, 100.0));
        // No crash
    }

    #[test]
    fn dormant_child_does_not_misalign_placement() {
        let mut tree = WidgetTree::new();
        let a = tree.add(FixedLeaf(40.0, 20.0));
        let b = tree.add(FixedLeaf(50.0, 25.0));
        let c = tree.add(FixedLeaf(60.0, 30.0));
        let _wrap = tree.add(
            Wrap::new()
                .spacing(10.0)
                .add_child(a)
                .add_child(b)
                .add_child(c),
        );
        tree.layout(SizeProposal::exact(300.0, 100.0));

        // Before dormant: a at x=0, b at x=50, c at x=110
        assert!((tree.bounds(b).x - 50.0).abs() < 0.01);
        assert!((tree.bounds(b).width - 50.0).abs() < 0.01);
        assert!((tree.bounds(c).x - 110.0).abs() < 0.01);
        assert!((tree.bounds(c).width - 60.0).abs() < 0.01);

        // Make first child dormant
        tree.set_dormant(a);
        tree.layout(SizeProposal::exact(300.0, 100.0));

        // b should get its own size (50x25), not a's size (40x20)
        assert!((tree.bounds(b).x - 0.0).abs() < 0.01);
        assert!((tree.bounds(b).width - 50.0).abs() < 0.01);
        assert!((tree.bounds(b).height - 25.0).abs() < 0.01);
        // c follows b
        assert!((tree.bounds(c).x - 60.0).abs() < 0.01); // 50 + 10
        assert!((tree.bounds(c).width - 60.0).abs() < 0.01);
        assert!((tree.bounds(c).height - 30.0).abs() < 0.01);
    }
}
