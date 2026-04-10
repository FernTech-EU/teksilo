use fern_canvas::{Rect, Size, SizeProposal};
use fern_core::accessibility::AccessNodeBuilder;
use fern_core::build_context::BuildContext;
use fern_core::widget::{LayoutContext, Widget, WidgetPlacement};
use fern_core::widget_builder::HandlerSet;
use fern_core::widget_id::WidgetId;

pub(crate) struct OverlayTrigger {
    child_id: Option<WidgetId>,
    pending_child: Option<Box<dyn Widget>>,
    pending_handlers: Option<HandlerSet>,
    name: Option<String>,
}

impl OverlayTrigger {
    pub(crate) fn new(child: Box<dyn Widget>, handlers: HandlerSet) -> Self {
        Self {
            child_id: None,
            pending_child: Some(child),
            pending_handlers: Some(handlers),
            name: None,
        }
    }

    pub(crate) fn name(mut self, name: impl Into<String>) -> Self {
        self.name = Some(name.into());
        self
    }
}

impl std::fmt::Debug for OverlayTrigger {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("OverlayTrigger")
            .field("name", &self.name)
            .finish()
    }
}

impl Widget for OverlayTrigger {
    fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
        if let Some(child) = self.pending_child.take() {
            self.child_id = Some(ctx.add_boxed(child));
        }
        if let Some(handlers) = self.pending_handlers.take() {
            ctx.apply_self_handlers(handlers);
        }
        self.children()
    }

    fn size_that_fits(&self, proposal: SizeProposal, ctx: &LayoutContext) -> Size {
        self.child_id
            .and_then(|id| ctx.child_size(id, proposal))
            .unwrap_or_else(|| proposal.resolve(0.0, 0.0))
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

    fn accessibility(&self, builder: &mut AccessNodeBuilder) {
        builder.set_role(fern_core::accesskit::Role::Button);
        if let Some(name) = &self.name {
            builder.set_name(name.as_str());
        }
    }

    fn children(&self) -> Vec<WidgetId> {
        self.child_id.into_iter().collect()
    }
}
