use fern_canvas::{Rect, Size, SizeProposal};
use fern_core::accessibility::AccessNodeBuilder;
use fern_core::build_context::BuildContext;
use fern_core::signal::Signal;
use fern_core::widget::{LayoutContext, PendingChild, Widget, WidgetPlacement};
use fern_core::widget_builder::HandlerSet;
use fern_core::widget_id::WidgetId;

pub(crate) struct OverlayTrigger {
    child_id: Option<WidgetId>,
    pending_child: Option<PendingChild>,
    pending_handlers: Option<HandlerSet>,
    name: Option<String>,
    /// Optional `has_popup` hint surfaced on this trigger's a11y
    /// node. Same role as Button's equivalent — used by Popover
    /// for the ARIA disclosure pattern.
    has_popup: Option<fern_core::accesskit::HasPopup>,
    /// Optional signal reporting whether the owned popup is
    /// currently visible. Published via `set_expanded`.
    expanded_signal: Option<Signal<bool>>,
}

impl OverlayTrigger {
    pub(crate) fn new(child: Box<dyn Widget>, handlers: HandlerSet) -> Self {
        Self::from_pending(PendingChild::Deferred(child), handlers)
    }

    pub(crate) fn from_id(id: WidgetId, handlers: HandlerSet) -> Self {
        Self::from_pending(PendingChild::Id(id), handlers)
    }

    fn from_pending(pending: PendingChild, handlers: HandlerSet) -> Self {
        Self {
            child_id: None,
            pending_child: Some(pending),
            pending_handlers: Some(handlers),
            name: None,
            has_popup: None,
            expanded_signal: None,
        }
    }

    pub(crate) fn name(mut self, name: impl Into<String>) -> Self {
        self.name = Some(name.into());
        self
    }

    pub(crate) fn has_popup(mut self, kind: fern_core::accesskit::HasPopup) -> Self {
        self.has_popup = Some(kind);
        self
    }

    pub(crate) fn expanded_when(mut self, signal: Signal<bool>) -> Self {
        self.expanded_signal = Some(signal);
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
        if let Some(pending) = self.pending_child.take() {
            self.child_id = Some(match pending {
                PendingChild::Id(id) => id,
                PendingChild::Deferred(w) => ctx.add_boxed(w),
            });
        }
        if let Some(handlers) = self.pending_handlers.take() {
            ctx.apply_self_handlers(handlers);
        }
        // Register the expanded_signal so flips trigger an a11y
        // refresh on this trigger node.
        if let Some(ref expanded_signal) = self.expanded_signal {
            let self_id = ctx.self_id();
            let registry = ctx.binding_registry();
            expanded_signal.bind_to(
                self_id,
                registry,
                fern_core::binding::BindingLevel::RepaintOnly,
            );
        }
        self.children()
    }

    fn layout_response(&self, proposal: SizeProposal, ctx: &LayoutContext) -> fern_core::widget::LayoutResponse {
        self.child_id
            .and_then(|id| ctx.child_size(id, proposal))
            .unwrap_or_else(|| proposal.resolve(0.0, 0.0)).into()
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
        if let Some(kind) = self.has_popup {
            builder.set_has_popup(kind);
        }
        if let Some(ref signal) = self.expanded_signal {
            builder.set_expanded(signal.get());
        }
    }

    fn children(&self) -> Vec<WidgetId> {
        self.child_id.into_iter().collect()
    }
}
