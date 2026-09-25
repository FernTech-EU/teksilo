// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! `OverlayTrigger` — the shared "this widget opens that overlay" wrapper.
//!
//! Used by `Dialog`, `Snackbar`, `Wizard` and `PopoverWidget` whenever a
//! caller replaces the default `Button` trigger with a widget of their own.
//!
//! ## One control, on whichever node takes focus
//!
//! To a screen reader a trigger is one control, and the node focus lands on is
//! that control: it carries the role, the name, the popup state, and it
//! answers Enter, Space and the AT `Click`. Which node that is depends on what
//! was wrapped, and is decided once, as it mounts:
//!
//! * **A widget that takes no focus** (a panel, a glyph, a label): the
//!   trigger's own node is the button. It is the Tab stop and it carries
//!   everything above.
//! * **A control of its own** (a `Button`, an `IconButton`, anything that is or
//!   holds a focus stop, even one disabled as it mounts): that control is the
//!   button. Its role and its own text stand, the opening routes and the popup
//!   state are added to it, and the trigger's node is structure, which the
//!   tree walk leaves out. The name given to the trigger is not used there:
//!   the control's own text is what a sighted user reads on it, and a second
//!   button around the first would be one control heard as two.
//!
//! ## Touch and pen
//!
//! The trigger has no geometry and no press visual of its own: it forwards the
//! caller's child, whose target and appearance are the child's, and routes the
//! pointer handler onto that child's external bucket so it fires beside the
//! child's own. The activation is an `on_tap`, so it happens on the release for
//! every pointer kind.

use std::rc::Rc;

use teksilo_canvas::{Rect, SizeProposal};
use teksilo_core::accessibility::AccessNodeBuilder;
use teksilo_core::accesskit::{Action, HasPopup, Role};
use teksilo_core::build_context::BuildContext;
use teksilo_core::event::{EventResponse, Key, WidgetEvent};
use teksilo_core::signal::{Prop, Signal};
use teksilo_core::widget::{
    CursorIcon, EventContext, LayoutContext, PendingChild, Widget, WidgetPlacement,
};
use teksilo_core::widget_builder::HandlerSet;
use teksilo_core::widget_id::WidgetId;

type Activate = Rc<dyn Fn(&mut EventContext)>;

/// Wraps an arbitrary widget so it can drive a popover.
///
/// `PopoverButton` and `PopoverIconButton` cover the two stock triggers; this
/// is the third case — a trigger that is *not* a button, such as a table
/// header's filter glyph or a tag chip. It supplies what those two get from
/// `Button`/`IconButton`: a Tab stop, an activate route (pointer, Enter/Space,
/// and the AT `Click` action), the `has_popup` / `expanded` disclosure
/// annotations, and the arena-level `enabled` gate. A wrapped widget that is a
/// control already keeps its own focus, role and name; see the module docs.
///
/// ```ignore
/// PopoverWidget::new(OverlayTrigger::around(my_glyph))
///     .content(my_panel)
///     .placement(OverlayPlacement::BelowPreferred)
/// ```
pub struct OverlayTrigger {
    child_id: Option<WidgetId>,
    pending_child: Option<PendingChild>,
    name: Option<String>,
    /// Optional `has_popup` hint surfaced on the trigger's control node.
    /// Same role as Button's equivalent: used by Popover for the ARIA
    /// disclosure pattern.
    has_popup: Option<HasPopup>,
    /// Optional signal reporting whether the owned popup is
    /// currently visible. Published via `set_expanded`.
    expanded_signal: Option<Signal<bool>>,
    /// Enabled state, wired into the arena on this trigger's node so
    /// a disabled custom trigger greys out (via `effective_enabled`),
    /// reports `disabled` to AT, and has its pointer/key dispatch
    /// gated — the same treatment a stock `Button` gets. Default
    /// `Prop::Static(true)`.
    enabled: Prop<bool>,
    /// What opening the overlay does. Installed by the presenter (`Dialog`,
    /// `Snackbar`, `Wizard`) or by
    /// [`crate::popover_widget::PopoverTrigger::with_on_activate`], and routed
    /// in `build` onto the child as a pointer tap and onto the control node as
    /// Enter/Space and the AT `Click`, so a custom trigger is reachable exactly
    /// the ways a `Button` trigger is.
    on_activate: Option<Activate>,
    /// Enter and Space open on the key's release rather than its press. The
    /// modal presenters ask for it; a popover opens on the press.
    activate_on_key_up: bool,
    /// The wrapped widget's own focus stop, when it has one. Set as the child
    /// mounts. `Some` makes that widget the control, and this node structure;
    /// `None` makes this node the control. See the module docs.
    wrapped_control: Option<WidgetId>,
}

impl OverlayTrigger {
    pub(crate) fn new(child: Box<dyn Widget>) -> Self {
        Self::from_pending(PendingChild::Deferred(child))
    }

    pub(crate) fn from_pending(pending: PendingChild) -> Self {
        Self {
            child_id: None,
            pending_child: Some(pending),
            name: None,
            has_popup: None,
            expanded_signal: None,
            enabled: Prop::Static(true),
            on_activate: None,
            activate_on_key_up: false,
            wrapped_control: None,
        }
    }

    /// Wrap any widget as a popover trigger.
    pub fn around(widget: impl Widget + 'static) -> Self {
        Self::from_pending(teksilo_core::IntoTeksiChild::into_pending(widget))
    }

    /// [`around`](Self::around) for a widget already inserted by id.
    pub fn around_id(id: WidgetId) -> Self {
        Self::from_pending(PendingChild::Id(id))
    }

    /// Set the trigger's accessible name.
    ///
    /// Used when the wrapped widget takes no focus of its own. A wrapped
    /// control keeps its own name.
    pub fn named(self, name: impl Into<String>) -> Self {
        self.name(name)
    }

    /// Whether an activate handler is already installed.
    pub fn has_on_activate(&self) -> bool {
        self.on_activate.is_some()
    }

    /// Install the overlay's open/close handler. Routed onto the wrapped widget
    /// as a pointer tap, and onto the node that takes focus as Enter/Space and
    /// the AT `Click` action.
    pub fn on_activate(mut self, f: impl Fn(&mut EventContext) + 'static) -> Self {
        self.on_activate = Some(Rc::new(f));
        self
    }

    /// Open on the release of Enter or Space rather than on the press.
    pub(crate) fn activate_on_key_up(mut self) -> Self {
        self.activate_on_key_up = true;
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

    pub(crate) fn has_popup(mut self, kind: HasPopup) -> Self {
        self.has_popup = Some(kind);
        self
    }

    pub(crate) fn expanded_when(mut self, signal: Signal<bool>) -> Self {
        self.expanded_signal = Some(signal);
        self
    }

    /// Enter/Space and the AT `Click`, for the node that takes focus.
    fn keyboard_and_at(&self, activate: Activate) -> HandlerSet {
        let on_key_up = self.activate_on_key_up;
        let key = activate.clone();
        HandlerSet::new()
            .on_key(move |event, ctx| match event {
                WidgetEvent::KeyDown {
                    key: Key::Enter | Key::Space,
                    ..
                } if !on_key_up => {
                    key(ctx);
                    EventResponse::Handled
                }
                WidgetEvent::KeyUp {
                    key: Key::Enter | Key::Space,
                    ..
                } if on_key_up => {
                    key(ctx);
                    EventResponse::Handled
                }
                _ => EventResponse::Ignored,
            })
            .on_access_action(move |action, ctx| {
                if action == Action::Click {
                    activate(ctx);
                    EventResponse::Handled
                } else {
                    EventResponse::Ignored
                }
            })
    }

    /// The popup state, written onto a wrapped control's own node after its
    /// widget has described itself.
    fn disclosure(&self) -> Option<impl Fn(&mut AccessNodeBuilder) + 'static> {
        if self.has_popup.is_none() && self.expanded_signal.is_none() {
            return None;
        }
        let has_popup = self.has_popup;
        let expanded = self.expanded_signal.clone();
        Some(move |builder: &mut AccessNodeBuilder| {
            if let Some(kind) = has_popup {
                builder.set_has_popup(kind);
            }
            if let Some(ref signal) = expanded {
                builder.set_expanded(signal.get());
            }
        })
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
            let child = match pending {
                PendingChild::Id(id) => id,
                PendingChild::Deferred(w) => ctx.add_boxed(w),
            };
            self.child_id = Some(child);
            // `add` builds the child's whole subtree before it returns, so
            // whether it is a control of its own is already known. A control
            // disabled right now is still the control: asking only for what
            // can take focus now would add a second, enabled stop beside it,
            // one that opens what the disabled control is there to withhold.
            self.wrapped_control = ctx.first_focus_capable_descendant(child);
        }
        if let Some(activate) = self.on_activate.clone() {
            // The pointer route goes onto the CHILD, not onto ourselves. The
            // child is the hit-test target and the first node in the bubble
            // pass. If it has its own gesture arena (a real `Button`, which
            // unconditionally wires `on_tap` for InteractionState tracking),
            // it consumes the tap before any ancestor can see it. In the
            // child's *external* bucket the opener fires alongside the child's
            // own handlers when the gesture arena emits `Tap`. For a
            // non-interactive child (a `Panel`, a glyph) `ensure_gesture_arena`
            // lazily installs a recognizer for it.
            let tap = activate.clone();
            let pointer = HandlerSet::new()
                .on_tap(move |_event, ctx| tap(ctx))
                .cursor(CursorIcon::Pointer);
            // Keys and the AT action go to the node that takes focus: key
            // events are dispatched there, and an `AccessAction` to the node a
            // reader is on, bubbling only if that node leaves it unhandled,
            // which a `Button` never does.
            let keys = self.keyboard_and_at(activate);
            match (self.child_id, self.wrapped_control) {
                (Some(child), Some(control)) => {
                    ctx.apply_handlers(child, pointer);
                    let keys = match self.disclosure() {
                        Some(disclosure) => keys.access_customize(disclosure),
                        None => keys,
                    };
                    ctx.apply_handlers(control, keys);
                }
                (Some(child), None) => {
                    ctx.apply_handlers(child, pointer);
                    ctx.apply_self_handlers(keys.focusable(true));
                }
                (None, _) => {
                    ctx.apply_self_handlers(pointer);
                    ctx.apply_self_handlers(keys.focusable(true));
                }
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
        if self.wrapped_control.is_some() {
            // The wrapped control is the button; this node is structure,
            // dropped by the walker and by every adapter with its child kept.
            builder.set_role(Role::GenericContainer);
            return;
        }
        builder.set_role(Role::Button);
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
        if self.on_activate.is_some() {
            builder.add_action(Action::Click);
        }
    }

    fn children(&self) -> Vec<WidgetId> {
        self.child_id.into_iter().collect()
    }

    /// A wrapped control stands for the trigger, so what is attached to the
    /// trigger reaches the node a reader is on: a popover's dialog, named by
    /// its trigger through `labelled_by`, is named by the control's text.
    fn accessibility_proxy(&self) -> Option<WidgetId> {
        self.wrapped_control
    }
}

#[cfg(test)]
mod reader_tests;
