// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! `OverlayTrigger` — the shared "this widget opens that overlay" wrapper.
//!
//! Used by `Dialog`, `Snackbar` and every other presenter that lets a caller
//! replace its default `Button` trigger with a widget of their own.
//!
//! ## Touch and pen
//!
//! The trigger has no geometry and no press visual of its own: it forwards the
//! caller's child, whose target and appearance are the child's, and routes the
//! opening handlers onto that child's external bucket so they fire beside the
//! child's own. The activation is an `on_tap`, so it happens on the release for
//! every pointer kind.

use teksilo_canvas::{Rect, SizeProposal};
use teksilo_core::accessibility::AccessNodeBuilder;
use teksilo_core::build_context::BuildContext;
use teksilo_core::signal::{Prop, Signal};
use teksilo_core::widget::{LayoutContext, PendingChild, Widget, WidgetPlacement};
use teksilo_core::widget_builder::HandlerSet;
use teksilo_core::widget_id::WidgetId;

/// Wraps an arbitrary widget so it can drive a popover.
///
/// `PopoverButton` and `PopoverIconButton` cover the two stock triggers; this
/// is the third case — a trigger that is *not* a button, such as a table
/// header's filter glyph or a tag chip. It supplies what those two get from
/// `Button`/`IconButton`: an activate route (pointer, Enter/Space, and the
/// AT `Click` action), the `has_popup` / `expanded` disclosure annotations, and
/// the arena-level `enabled` gate.
///
/// ```ignore
/// PopoverWidget::new(OverlayTrigger::around(my_glyph))
///     .content(my_panel)
///     .placement(OverlayPlacement::BelowPreferred)
/// ```
pub struct OverlayTrigger {
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
    /// Installed by [`crate::popover_widget::PopoverTrigger::with_on_activate`]. Routed onto the child
    /// in `build` as pointer-tap and Enter/Space, and onto *this* node as the
    /// AT `Click` action, so a custom trigger is reachable exactly the ways a
    /// `Button` trigger is.
    on_activate: Option<std::rc::Rc<dyn Fn(&mut teksilo_core::widget::EventContext)>>,
    /// The AT `Click` route for a presenter that builds its own
    /// [`HandlerSet`] (`Dialog`, `Snackbar`) rather than going through
    /// [`on_activate`](Self::on_activate).
    ///
    /// It has to be a *separate* setter, and it has to land on this node
    /// rather than on the child, because this is the node that carries
    /// `Role::Button`: an AT action dispatches to the node it was invoked on
    /// and then bubbles towards the root, so a handler parked on the child —
    /// a descendant — is never on its path. A presenter that put its AT
    /// handler in the child's `HandlerSet` therefore published a named,
    /// correctly-roled button that no screen reader could activate.
    on_access_activate: Option<std::rc::Rc<dyn Fn(&mut teksilo_core::widget::EventContext)>>,
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
            on_activate: None,
            on_access_activate: None,
        }
    }

    /// Wrap any widget as a popover trigger.
    pub fn around(widget: impl Widget + 'static) -> Self {
        Self::from_pending(
            teksilo_core::IntoTeksiChild::into_pending(widget),
            HandlerSet::new(),
        )
    }

    /// [`around`](Self::around) for a widget already inserted by id.
    pub fn around_id(id: WidgetId) -> Self {
        Self::from_pending(PendingChild::Id(id), HandlerSet::new())
    }

    /// Set the trigger's accessible name.
    pub fn named(self, name: impl Into<String>) -> Self {
        self.name(name)
    }

    /// Whether an activate handler is already installed.
    pub fn has_on_activate(&self) -> bool {
        self.on_activate.is_some()
    }

    /// Install the popover's open/close handler. Routed onto the wrapped widget
    /// as pointer-tap and Enter/Space, and onto this trigger's own node as the
    /// AT `Click` action.
    pub fn on_activate(
        mut self,
        f: impl Fn(&mut teksilo_core::widget::EventContext) + 'static,
    ) -> Self {
        self.on_activate = Some(std::rc::Rc::new(f));
        self
    }

    /// Install *only* the AT `Click` route, on this trigger's own node.
    ///
    /// For a presenter that hands the pointer and keyboard routes over in its
    /// own [`HandlerSet`] (which is applied to the child, so the child's
    /// gesture arena cannot swallow them first) but still needs the AT action
    /// on the node that carries `Role::Button`. See
    /// [`on_access_activate`](Self::on_access_activate)'s field docs for why
    /// the two cannot share a destination.
    pub(crate) fn on_access_activate(
        mut self,
        f: impl Fn(&mut teksilo_core::widget::EventContext) + 'static,
    ) -> Self {
        self.on_access_activate = Some(std::rc::Rc::new(f));
        self
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
        let mut handlers = self.pending_handlers.take();
        if let Some(activate) = self.on_activate.clone() {
            let set = handlers.take().unwrap_or_default();
            let tap = activate.clone();
            let key = activate.clone();
            handlers = Some(
                set.on_tap(move |_pos, ctx| tap(ctx))
                    .on_key(move |event, ctx| match event {
                        teksilo_core::event::WidgetEvent::KeyDown {
                            key: teksilo_core::event::Key::Enter | teksilo_core::event::Key::Space,
                            ..
                        } => {
                            key(ctx);
                            teksilo_core::event::EventResponse::Handled
                        }
                        _ => teksilo_core::event::EventResponse::Ignored,
                    }),
            );
            // The AT route splits off here and lands on SELF: an
            // `AccessAction` dispatches to the node it was invoked on — this
            // one, the node `accessibility` gives `Role::Button` — and then
            // bubbles rootwards, so the child never sees it. There is no
            // gesture arena to lose it to either, which is the whole reason
            // tap and key go the other way.
            if self.on_access_activate.is_none() {
                self.on_access_activate = Some(activate);
            }
        }
        if let Some(handlers) = handlers {
            if let Some(child_id) = self.child_id {
                ctx.apply_handlers(child_id, handlers);
            } else {
                // No child — keep handlers on self so they aren't lost.
                ctx.apply_self_handlers(handlers);
            }
        }
        if let Some(activate) = self.on_access_activate.clone() {
            ctx.apply_self_handlers(HandlerSet::new().on_access_action(move |action, ctx| {
                if action == teksilo_core::accesskit::Action::Click {
                    activate(ctx);
                    teksilo_core::event::EventResponse::Handled
                } else {
                    teksilo_core::event::EventResponse::Ignored
                }
            }));
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
        // Advertise what this node can actually do. The handler alone is not
        // enough: AccessKit consumers read the action list, `accesskit`'s own
        // platform adapters refuse an unadvertised action, and an audit that
        // only checks names and roles passes a button no screen reader can
        // press. Only claimed when there is a route to claim — a bare
        // `OverlayTrigger::around(w)` that no presenter has wired up yet
        // advertises nothing, which is the truth about it.
        if self.on_access_activate.is_some() {
            builder.add_action(teksilo_core::accesskit::Action::Click);
        }
    }

    fn children(&self) -> Vec<WidgetId> {
        self.child_id.into_iter().collect()
    }
}
