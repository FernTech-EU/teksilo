// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! MaxSize — a layout modifier that caps a child to a maximum width and/or height.
//!
//! The child is proposed the lesser of the parent's proposal and the configured
//! maximum on each axis; the reported size is then clamped again so a child that
//! intrinsically overshoots the cap is always contained. Axes with no maximum set
//! are passed through unchanged.
//!
//! `MaxSize` clips its child when a maximum is active (`clips_children() == true`)
//! so content that still overflows after layout does not bleed into adjacent widgets.
//! Maximum values can be static or bound to a reactive [`Signal<f32>`](bastyde_core::signal::Signal)
//! for animated or data-driven constraints.
//!
//! For the inverse operation (ensuring a minimum size) see [`MinSize`](super::MinSize).
//!
//! ```rust
//! # use bastyde_widgets::primitives::{MaxSize, TextWidget};
//! # use bastyde_i18n::lit;
//! // Cap a text widget to 240 logical pixels wide.
//! let _w = MaxSize::width(240.0)
//!     .child(TextWidget::new(lit!("This text will not exceed 240 dp.")));
//! ```

use bastyde_canvas::{Rect, Size, SizeProposal};
use bastyde_core::accessibility::AccessNodeBuilder;
use bastyde_core::signal::Prop;
use bastyde_core::widget::{LayoutContext, PaintContext, PendingChild, Widget, WidgetPlacement};
use bastyde_core::widget_id::WidgetId;

/// Layout modifier that enforces a maximum width and/or height on a single child widget.
///
/// Constraints can be static or bound to a reactive `Signal<f32>` for dynamic resizing.
#[derive(Debug)]
pub struct MaxSize {
    child_id: Option<WidgetId>,
    pending_child: Option<PendingChild>,
    max_width: Option<Prop<f32>>,
    max_height: Option<Prop<f32>>,
}

impl MaxSize {
    /// Cap both axes: the child's width will not exceed `width` and its height will not exceed `height`.
    pub fn new(width: f32, height: f32) -> Self {
        Self {
            child_id: None,
            pending_child: None,
            max_width: Some(Prop::Static(width)),
            max_height: Some(Prop::Static(height)),
        }
    }

    /// Cap only the width axis; the height axis is unconstrained by this modifier.
    pub fn width(width: f32) -> Self {
        Self {
            child_id: None,
            pending_child: None,
            max_width: Some(Prop::Static(width)),
            max_height: None,
        }
    }

    /// Cap only the height axis; the width axis is unconstrained by this modifier.
    pub fn height(height: f32) -> Self {
        Self {
            child_id: None,
            pending_child: None,
            max_width: None,
            max_height: Some(Prop::Static(height)),
        }
    }

    /// Bind max width to a reactive state.
    pub fn max_width(mut self, state: impl Into<Prop<f32>>) -> Self {
        self.max_width = Some(state.into());
        self
    }

    /// Bind max height to a reactive state.
    pub fn max_height(mut self, state: impl Into<Prop<f32>>) -> Self {
        self.max_height = Some(state.into());
        self
    }

    /// Set child by pre-registered ID.
    pub fn child_id(mut self, id: WidgetId) -> Self {
        self.pending_child = Some(PendingChild::Id(id));
        self
    }

    /// Set an inline child widget (deferred insertion).
    pub fn child(mut self, widget: impl Widget + 'static) -> Self {
        self.pending_child = Some(PendingChild::Deferred(Box::new(widget)));
        self
    }
}

impl Widget for MaxSize {
    fn build(&mut self, ctx: &mut bastyde_core::build_context::BuildContext) -> Vec<WidgetId> {
        if let Some(pending) = self.pending_child.take() {
            self.child_id = Some(match pending {
                PendingChild::Id(id) => id,
                PendingChild::Deferred(w) => ctx.add_boxed(w),
            });
        }
        let self_id = ctx.self_id();
        let registry = ctx.binding_registry();
        if let Some(ref w) = self.max_width {
            w.register_if_bound(
                self_id,
                registry,
                bastyde_core::binding::BindingLevel::Relayout,
            );
        }
        if let Some(ref h) = self.max_height {
            h.register_if_bound(
                self_id,
                registry,
                bastyde_core::binding::BindingLevel::Relayout,
            );
        }
        self.child_id.into_iter().collect()
    }

    fn layout_response(
        &self,
        proposal: SizeProposal,
        ctx: &LayoutContext,
    ) -> bastyde_core::widget::LayoutResponse {
        let max_w = self.max_width.as_ref().map(|r| r.get());
        let max_h = self.max_height.as_ref().map(|r| r.get());

        let clamped_proposal = SizeProposal {
            width: match (proposal.width, max_w) {
                (Some(w), Some(max)) => Some(w.min(max)),
                (None, Some(max)) => Some(max),
                (w, None) => w,
            },
            height: match (proposal.height, max_h) {
                (Some(h), Some(max)) => Some(h.min(max)),
                (None, Some(max)) => Some(max),
                (h, None) => h,
            },
        };

        let child_size = self
            .child_id
            .and_then(|id| ctx.child_size(id, clamped_proposal))
            .unwrap_or(Size::ZERO);

        let w = match max_w {
            Some(max) => child_size.width.min(max),
            None => child_size.width,
        };
        let h = match max_h {
            Some(max) => child_size.height.min(max),
            None => child_size.height,
        };
        Size::new(w, h).into()
    }

    fn place_children(
        &self,
        bounds: Rect,
        _proposal: SizeProposal,
        children: &mut [WidgetPlacement],
        _ctx: &LayoutContext,
    ) {
        for child in children.iter_mut() {
            child.origin = bounds.origin();
            child.size = bounds.size();
        }
    }

    fn paint(&self, _bounds: Rect, _canvas: &mut bastyde_canvas::Canvas, _ctx: &PaintContext) {}

    fn clips_children(&self) -> bool {
        self.max_width.is_some() || self.max_height.is_some()
    }

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
    use bastyde_core::signal::Signal;
    use bastyde_core::widget_tree::WidgetTree;

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

    #[test]
    fn clamps_large_child_to_maximum() {
        let mut tree = WidgetTree::new();
        let child = tree.add(FixedLeaf(800.0, 600.0));
        let max = tree.add(MaxSize::new(400.0, 300.0).child_id(child));
        tree.layout(SizeProposal::unspecified());

        let mb = tree.bounds(max);
        assert!((mb.width - 400.0).abs() < 0.01);
        assert!((mb.height - 300.0).abs() < 0.01);
    }

    #[test]
    fn small_child_is_not_clamped() {
        let mut tree = WidgetTree::new();
        let child = tree.add(FixedLeaf(100.0, 50.0));
        let max = tree.add(MaxSize::new(400.0, 300.0).child_id(child));
        tree.layout(SizeProposal::unspecified());

        let mb = tree.bounds(max);
        assert!((mb.width - 100.0).abs() < 0.01);
        assert!((mb.height - 50.0).abs() < 0.01);
    }

    #[test]
    fn max_width_only() {
        let mut tree = WidgetTree::new();
        let child = tree.add(FixedLeaf(800.0, 50.0));
        let max = tree.add(MaxSize::width(400.0).child_id(child));
        tree.layout(SizeProposal::unspecified());

        let mb = tree.bounds(max);
        assert!((mb.width - 400.0).abs() < 0.01);
        assert!((mb.height - 50.0).abs() < 0.01);
    }

    #[test]
    fn max_width_dynamic() {
        let max_w = Signal::new(400.0_f32);
        let mut tree = WidgetTree::new();
        let child = tree.add(FixedLeaf(800.0, 50.0));
        let max = tree.add(
            MaxSize::width(9999.0)
                .max_width(max_w.clone())
                .child_id(child),
        );
        tree.layout(SizeProposal::unspecified());
        assert!((tree.bounds(max).width - 400.0).abs() < 0.01);

        max_w.set(200.0);
        tree.layout(SizeProposal::unspecified());
        assert!((tree.bounds(max).width - 200.0).abs() < 0.01);
    }
}
