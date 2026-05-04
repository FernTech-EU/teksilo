//! `FillWidthFixedHeight` — layout helper for the inspector panel
//! slot.
//!
//! The framework's `FixedSize` is designed to *pin* both axes and
//! reports its child's natural size when an axis is unbound, ignoring
//! the parent's proposal. That's the wrong shape for the inspector
//! panel: the panel needs a reactive height (driven by the resize
//! handle) AND must fill the window width. With plain
//! `FixedSize::new().bind_height(h)` the panel ends up at its
//! contents' intrinsic width, leaving the right portion of the
//! window blank.
//!
//! This widget binds height to a `Signal<f32>` and reports
//! `proposal.width` as its own width — passing both through to its
//! child as an exact proposal so children that respect proposals
//! (like `ZStack` / `Panel`) place themselves at the full width.

use fern_canvas::{Rect, Size, SizeProposal};
use fern_core::accessibility::AccessNodeBuilder;
use fern_core::binding::BindingLevel;
use fern_core::build_context::BuildContext;
use fern_core::signal::Signal;
use fern_core::widget::{LayoutContext, LayoutResponse, Widget, WidgetPlacement};
use fern_core::widget_id::WidgetId;

pub(crate) struct FillWidthFixedHeight {
    height: Signal<f32>,
    child: Option<Box<dyn Widget>>,
    child_id: Option<WidgetId>,
}

impl FillWidthFixedHeight {
    pub fn new(height: Signal<f32>, child: impl Widget + 'static) -> Self {
        Self {
            height,
            child: Some(Box::new(child)),
            child_id: None,
        }
    }
}

impl std::fmt::Debug for FillWidthFixedHeight {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FillWidthFixedHeight").finish()
    }
}

impl Widget for FillWidthFixedHeight {
    fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
        let self_id = ctx.self_id();
        self.height
            .bind_to(self_id, ctx.binding_registry(), BindingLevel::Relayout);
        if let Some(child) = self.child.take() {
            let id = ctx.add_boxed(child);
            self.child_id = Some(id);
            vec![id]
        } else {
            Vec::new()
        }
    }

    fn layout_response(&self, proposal: SizeProposal, ctx: &LayoutContext) -> LayoutResponse {
        let h = self.height.get().max(0.0);
        let w = proposal.width.unwrap_or(0.0);
        if let Some(cid) = self.child_id {
            // Forward the resolved size to the child so children that
            // respect their proposal (e.g. ZStack, Panel) actually
            // fill it.
            let child_proposal = SizeProposal {
                width: Some(w),
                height: Some(h),
            };
            let _ = ctx.child_size(cid, child_proposal);
        }
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

    fn accessibility(&self, _builder: &mut AccessNodeBuilder) {}
}
