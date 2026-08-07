// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

use teksilo_canvas::{Rect, SizeProposal};
use teksilo_core::accessibility::AccessNodeBuilder;
use teksilo_core::build_context::BuildContext;
use teksilo_core::signal::{Prop, Signal};
use teksilo_core::widget::{LayoutContext, PendingChild, Widget, WidgetPlacement};
use teksilo_core::widget_builder::HandlerSet;
use teksilo_core::widget_id::WidgetId;

pub(crate) struct OverlayTrigger {
    child_id: Option<WidgetId>,
    pending_child: Option<PendingChild>,
    pending_handlers: Option<HandlerSet>,
    name: Option<String>,
    /// Optional `has_popup` hint surfaced on this trigger's a11y
    /// node. Same role as Button's equivalent — used by Popover
    /// for the ARIA disclosure pattern.
    has_popup: Option<teksilo_core::accesskit::HasPopup>,
    /// Optional signal reporting whether the owned popup is
    /// currently visible. Published via `set_expanded`.
    expanded_signal: Option<Signal<bool>>,
    /// Enabled state, wired into the arena on this trigger's node so
    /// a disabled custom trigger greys out (via `effective_enabled`),
    /// reports `disabled` to AT, and has its pointer/key dispatch
    /// gated — the same treatment a stock `Button` gets. Default
    /// `Prop::Static(true)`.
    enabled: Prop<bool>,
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
            enabled: Prop::Static(true),
        }
    }

    /// Set the trigger's enabled state (static or reactive). When
    /// `false`, the trigger child greys out, reports `disabled` to
    /// AT, and stops accepting pointer/key dispatch — via the arena's
    /// `enabled_when` cascade onto this node.
    pub(crate) fn enabled(mut self, enabled: impl Into<Prop<bool>>) -> Self {
        self.enabled = enabled.into();
        self
    }

    pub(crate) fn name(mut self, name: impl Into<String>) -> Self {
        self.name = Some(name.into());
        self
    }

    pub(crate) fn has_popup(mut self, kind: teksilo_core::accesskit::HasPopup) -> Self {
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
        // Wire enabled into the arena on this trigger node. The child is
        // a descendant, so `arena.is_enabled` (ancestor walk) gates its
        // dispatch, `effective_enabled` greys it out, and the a11y walker
        // marks it disabled — with no per-trigger bool snapshot.
        let self_id = ctx.self_id();
        ctx.enabled_when(self_id, self.enabled.clone());
        if let Some(pending) = self.pending_child.take() {
            self.child_id = Some(match pending {
                PendingChild::Id(id) => id,
                PendingChild::Deferred(w) => ctx.add_boxed(w),
            });
        }
        // Attach handlers to the CHILD, not to ourselves. The child is
        // the hit-test target and the first node in the bubble pass —
        // if it has its own gesture arena (e.g. a real `Button`, which
        // unconditionally wires `on_tap` for InteractionState
        // tracking), it consumes the tap before any ancestor can see
        // it. Routing the overlay-opening handlers onto the child's
        // *external* bucket means they fire alongside the child's own
        // handlers when the gesture arena emits `Tap`.
        //
        // For non-interactive triggers (test `FixedLeaf`, `Panel`,
        // etc.) `ensure_gesture_arena` lazily installs a recognizer
        // for the external `on_tap`, so the same path works.
        if let Some(handlers) = self.pending_handlers.take() {
            if let Some(child_id) = self.child_id {
                ctx.apply_handlers(child_id, handlers);
            } else {
                // No child — keep handlers on self so they aren't lost.
                ctx.apply_self_handlers(handlers);
            }
        }
        // Register the expanded_signal so flips trigger an a11y
        // refresh on this trigger node.
        if let Some(ref expanded_signal) = self.expanded_signal {
            let registry = ctx.binding_registry();
            expanded_signal.bind_to(
                self_id,
                registry,
                teksilo_core::binding::BindingLevel::RepaintOnly,
            );
        }
        self.children()
    }

    fn layout_response(
        &self,
        proposal: SizeProposal,
        ctx: &LayoutContext,
    ) -> teksilo_core::widget::LayoutResponse {
        self.child_id
            .and_then(|id| ctx.child_size(id, proposal))
            .unwrap_or_else(|| proposal.resolve(0.0, 0.0))
            .into()
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
        builder.set_role(teksilo_core::accesskit::Role::Button);
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
