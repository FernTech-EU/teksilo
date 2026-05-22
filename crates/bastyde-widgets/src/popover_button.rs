//! `PopoverButton` — a [`Button`] that opens a popover when activated.
//!
//! Wraps a caller-built [`Button`] with overlay wiring: owns a
//! `popover_open: Signal<bool>` toggled on activate / dismiss, sets
//! `has_popup` and `expanded_when` on the inner Button so AT
//! announces the disclosure state, pre-builds the popover content as
//! a dormant subtree, and shows / hides it via [`OverlayRequest`].
//! Mirrors [`DateEdit`](crate::date_edit::DateEdit)'s overlay wiring
//! verbatim — same `set_dormant` + `activate` + `show_overlay`
//! sequence, same `OverlayDismissCallback` shape — so behavior across
//! the family stays consistent.
//!
//! Replaces ad-hoc Button + Popover compositions in widgets like
//! [`ColorEdit`](crate::color_edit::ColorEdit). Suitable wherever a
//! disclosure trigger needs the standard Button chrome (focus halo,
//! interaction states, theme variants) plus a popover surface.
//!
//! ```ignore
//! use bastyde::widgets::{Button, ButtonVariant, PopoverButton, IconWidget};
//!
//! PopoverButton::new(
//!     Button::new(lit!("Choose…"))
//!         .variant(ButtonVariant::Plain)
//!         .trailing(IconWidget::chevron_down(12.0).access_hidden(true)),
//! )
//! .content(my_picker_widget)
//! .placement(OverlayPlacement::BelowPreferred)
//! ```
//!
//! # Trigger configuration overrides
//!
//! `PopoverButton::build()` configures the inner Button by calling:
//!
//! - `.has_popup(self.has_popup)` — defaults to
//!   [`HasPopup::Dialog`](bastyde_core::accesskit::HasPopup::Dialog).
//! - `.expanded_when(popover_open)` — drives the AT `set_expanded`
//!   announcement.
//! - `.on_activate_fn(...)` — toggles the popover.
//!
//! These three calls **replace** any previous values the caller set
//! on the trigger Button. In particular, any `on_activate_fn` set on
//! the Button before passing it to `PopoverButton::new` is discarded
//! — the activate slot is owned by the popover wiring. Apps that
//! need a side-effect on open / close should use [`Self::on_open`] /
//! [`Self::on_close`], or observe [`Self::open_signal`] from a
//! `ctx.effect`.

use std::rc::Rc;
use std::time::Duration;

use bastyde_canvas::{Rect, SizeProposal};
use bastyde_core::accessibility::AccessNodeBuilder;
use bastyde_core::accesskit::HasPopup;
use bastyde_core::build_context::BuildContext;
use bastyde_core::overlay::{
    DismissBehavior, OverlayDismissCallback, OverlayLayer, OverlayPlacement, OverlayRequest,
};
use bastyde_core::signal::Signal;
use bastyde_core::widget::{EventContext, LayoutContext, LayoutResponse, Widget, WidgetPlacement};
use bastyde_core::widget_id::WidgetId;

use crate::button::{Button, InteractionState, resolve_text_role};
use crate::popover_caret::DisclosureCaret;
use crate::primitives::ZStack;

type OnVoid = Rc<dyn Fn()>;

/// A `Button` paired with a popover surface.
///
/// See module docs for the contract on which Button properties get
/// overridden during `build()`.
pub struct PopoverButton {
    trigger: Option<Button>,
    content: Option<Box<dyn Widget>>,

    popover_open: Signal<bool>,
    placement: OverlayPlacement,
    dismiss_behavior: DismissBehavior,
    fade_duration: Option<Duration>,
    has_popup: HasPopup,
    show_disclosure_caret: bool,

    on_open: Option<OnVoid>,
    on_close: Option<OnVoid>,

    content_id: Option<WidgetId>,
    root_child_id: Option<WidgetId>,
}

impl std::fmt::Debug for PopoverButton {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PopoverButton")
            .field("placement", &self.placement)
            .field("dismiss_behavior", &self.dismiss_behavior)
            .field("has_popup", &self.has_popup)
            .field("popover_open", &self.popover_open.get())
            .finish_non_exhaustive()
    }
}

impl PopoverButton {
    /// Build a PopoverButton wrapping a pre-configured Button. The
    /// popover content is set separately via [`Self::content`].
    pub fn new(trigger: Button) -> Self {
        Self {
            trigger: Some(trigger),
            content: None,
            popover_open: Signal::new(false),
            placement: OverlayPlacement::BelowPreferred,
            dismiss_behavior: DismissBehavior::EscapeOrClickOutside,
            fade_duration: None,
            has_popup: HasPopup::Dialog,
            show_disclosure_caret: false,
            on_open: None,
            on_close: None,
            content_id: None,
            root_child_id: None,
        }
    }

    /// Paint a small downward-right disclosure triangle in the
    /// bottom-right corner of the trigger to advertise the popup.
    /// Default `false` — text buttons typically advertise their menu
    /// via an inline `.trailing(IconWidget::chevron_down(...))` chevron
    /// instead. Opt in for the JetBrains-style corner caret look (the
    /// same affordance used by
    /// [`PopoverIconButton`](crate::popover_icon_button::PopoverIconButton),
    /// which has it on by default since icon-only triggers have no
    /// label slot to host an inline chevron).
    ///
    /// The caret tints with the trigger's label color across hover /
    /// press / focus / disabled states (it shares the trigger's
    /// interaction signal via [`Button::share_interaction`]).
    /// AT-hidden — the popover affordance is announced via
    /// `set_has_popup` + `set_expanded` on the underlying Button.
    pub fn show_disclosure_caret(mut self, on: bool) -> Self {
        self.show_disclosure_caret = on;
        self
    }

    /// Set the popover content — added to the tree as a dormant
    /// subtree during `build()`, woken via
    /// [`EventContext::activate`](bastyde_core::widget::EventContext::activate)
    /// when the trigger fires. Required.
    pub fn content(mut self, content: impl Widget + 'static) -> Self {
        self.content = Some(Box::new(content));
        self
    }

    /// Override the popover's placement relative to the trigger.
    /// Default: [`OverlayPlacement::BelowPreferred`].
    pub fn placement(mut self, p: OverlayPlacement) -> Self {
        self.placement = p;
        self
    }

    /// Override the dismiss behavior. Default:
    /// [`DismissBehavior::EscapeOrClickOutside`].
    pub fn dismiss_behavior(mut self, b: DismissBehavior) -> Self {
        self.dismiss_behavior = b;
        self
    }

    /// Animate the overlay in / out over the given duration. Default:
    /// no fade. See [`OverlayRequest::with_fade`] for the underlying
    /// mechanism.
    pub fn fade_duration(mut self, d: Duration) -> Self {
        self.fade_duration = Some(d);
        self
    }

    /// Override the `has_popup` kind announced by AT. Default:
    /// [`HasPopup::Dialog`].
    pub fn has_popup_kind(mut self, k: HasPopup) -> Self {
        self.has_popup = k;
        self
    }

    /// Notification fired on the rising edge of the popover (after
    /// the overlay show request is dispatched). No `EventContext` —
    /// observe [`Self::open_signal`] in your `build()` if you need
    /// frame / dispatch context.
    pub fn on_open(mut self, f: impl Fn() + 'static) -> Self {
        self.on_open = Some(Rc::new(f));
        self
    }

    /// Notification fired on the falling edge of the popover (when
    /// the overlay's dismiss callback runs).
    pub fn on_close(mut self, f: impl Fn() + 'static) -> Self {
        self.on_close = Some(Rc::new(f));
        self
    }

    /// Observe-only handle to the popover-open state. Apps can
    /// `ctx.effect(&pb.open_signal(), ...)` from their composite to
    /// react with full `EventContext` — `on_open` / `on_close` on
    /// PopoverButton itself are notification-only (no ctx).
    pub fn open_signal(&self) -> Signal<bool> {
        self.popover_open.clone()
    }
}

impl Widget for PopoverButton {
    fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
        let content = self
            .content
            .take()
            .expect("PopoverButton::content(...) was not set");
        let content_id = ctx.add_boxed(content);
        ctx.set_dormant(content_id);
        self.content_id = Some(content_id);

        let trigger = self
            .trigger
            .take()
            .expect("PopoverButton::new must be called with a Button");

        let popover_open = self.popover_open.clone();
        let self_ref = ctx.self_id();
        let placement = self.placement.clone();
        let dismiss_behavior = self.dismiss_behavior.clone();
        let fade_duration = self.fade_duration;
        let on_open = self.on_open.clone();
        let on_close = self.on_close.clone();

        // Dismiss callback — runs when the overlay manager closes the
        // overlay (Escape, click-outside, or explicit dismiss). Flips
        // popover_open and fires the user's on_close. The callback
        // signature is `Rc<dyn Fn()>` — no `EventContext` available
        // here, so on_close is `Fn()` to match.
        let dismiss_cb: OverlayDismissCallback = {
            let popover_open = popover_open.clone();
            let on_close = on_close.clone();
            Rc::new(move || {
                popover_open.set(false);
                if let Some(cb) = on_close.as_ref() {
                    cb();
                }
            })
        };

        // Activate handler installed onto the inner Button. Toggles
        // the popover: if open, dismiss; if closed, wake the dormant
        // content, request the overlay, and move focus into it.
        let activate = {
            let popover_open = popover_open.clone();
            let dismiss_cb = dismiss_cb.clone();
            let on_open = on_open.clone();
            move |ctx_evt: &mut EventContext| {
                if popover_open.get() {
                    popover_open.set(false);
                    ctx_evt.dismiss_all_except_hosts();
                } else {
                    popover_open.set(true);
                    ctx_evt.activate(content_id);
                    let mut req = OverlayRequest {
                        content_id,
                        anchor: self_ref,
                        placement: placement.clone(),
                        dismiss: dismiss_behavior.clone(),
                        layer: OverlayLayer::InTree,
                        parent_overlay: None,
                        on_dismiss: Some(dismiss_cb.clone()),
                        fade_duration: None,
                    };
                    if let Some(d) = fade_duration {
                        req = req.with_fade(d);
                    }
                    ctx_evt.show_overlay(req);
                    ctx_evt.request_focus(content_id);
                    if let Some(cb) = on_open.as_ref() {
                        cb();
                    }
                }
            }
        };

        // When a disclosure caret is wanted, allocate the interaction
        // signal up-front and share it with the trigger so the
        // caret's color tracks the label's exactly. Without a caret,
        // Button allocates its own signal as before.
        if self.show_disclosure_caret {
            let variant = trigger.current_variant();
            let interaction = ctx.signal(InteractionState::Idle);
            let role_signal = interaction.map(move |s| resolve_text_role(variant, *s));
            let trigger = trigger
                .share_interaction(interaction)
                .has_popup(self.has_popup)
                .expanded_when(popover_open.clone())
                .on_activate_fn(activate);
            let trigger_id = ctx.add(trigger);
            let caret_id = ctx.add(DisclosureCaret { role: role_signal });
            let root_id = ctx.add(ZStack::new().add_child(trigger_id).add_child(caret_id));
            self.root_child_id = Some(root_id);
            // Return BOTH the trigger root AND the dormant content as
            // children so the framework links content_id under
            // `PopoverButton` in the arena. Without this, content_id
            // stays as an orphan root and `arena.hit_test_at` walks
            // its subtree on every click (even though `set_dormant`
            // ran on `content_id` itself, descendants added during the
            // content's own build can re-surface as hit targets at
            // their pre-dormant positions). The framework's layout
            // pass skips dormant children automatically.
            return vec![root_id, content_id];
        }

        let trigger = trigger
            .has_popup(self.has_popup)
            .expanded_when(popover_open.clone())
            .on_activate_fn(activate);

        let trigger_id = ctx.add(trigger);
        self.root_child_id = Some(trigger_id);
        // See comment above on the disclosure-caret branch — same
        // rationale for linking `content_id` as a child here.
        vec![trigger_id, content_id]
    }

    fn layout_response(&self, proposal: SizeProposal, ctx: &LayoutContext) -> LayoutResponse {
        match self.root_child_id {
            Some(id) => ctx
                .child_layout_response(id, proposal)
                .unwrap_or_else(|| proposal.resolve(0.0, 0.0).into()),
            None => proposal.resolve(0.0, 0.0).into(),
        }
    }

    fn place_children(
        &self,
        bounds: Rect,
        _proposal: SizeProposal,
        children: &mut [WidgetPlacement],
        _ctx: &LayoutContext,
    ) {
        // The trigger fills our bounds; the dormant/active content
        // never participates in trigger layout — its bounds are owned
        // by the overlay manager (via `position_overlays`) when shown
        // and stay at zero while dormant. Dormant children are already
        // filtered out by `layout_widget_recursive` before placements
        // reach this function; if the content is *active* (popover
        // open), zero its placement so the parent's bounds don't
        // accidentally drive a layout pass that would clobber the
        // overlay positioning.
        for child in children.iter_mut() {
            if Some(child.id) == self.content_id {
                child.size = bastyde_canvas::Size::ZERO;
                continue;
            }
            child.origin = bounds.origin();
            child.size = bounds.size();
        }
    }

    fn children(&self) -> Vec<WidgetId> {
        // Include both the trigger root AND the dormant content so
        // `set_dormant` cascades correctly and `arena.hit_test_at`
        // can prune the content subtree when it's not visible.
        let mut out = Vec::new();
        if let Some(id) = self.root_child_id {
            out.push(id);
        }
        if let Some(id) = self.content_id {
            out.push(id);
        }
        out
    }

    fn accessibility(&self, _builder: &mut AccessNodeBuilder) {
        // PopoverButton has no AT presence of its own — the inner
        // Button declares `Role::Button`, `set_has_popup`, and
        // `set_expanded`. The popover content advertises whatever
        // role / live region it wants. Nothing to add here.
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::button::Button;
    use crate::primitives::{MinSize, RectWidget};
    use bastyde_canvas::SizeProposal;
    use bastyde_core::accessibility::widget_id_to_node_id;
    use bastyde_core::accesskit::Role;
    use bastyde_core::event::{Key, Modifiers, WidgetEvent};
    use bastyde_core::widget_tree::WidgetTree;
    use bastyde_i18n::lit;

    #[test]
    fn build_does_not_panic_with_minimum_config() {
        let mut tree = WidgetTree::new().with_theme(bastyde_core::presets::intui::light());
        tree.add(
            PopoverButton::new(Button::new(lit!("Open")))
                .content(MinSize::new(40.0, 40.0).child(RectWidget::new())),
        );
        tree.layout(SizeProposal::exact(300.0, 80.0));
    }

    #[test]
    #[should_panic(expected = "PopoverButton::content")]
    fn build_panics_without_content() {
        let mut tree = WidgetTree::new().with_theme(bastyde_core::presets::intui::light());
        tree.add(PopoverButton::new(Button::new(lit!("Open"))));
        tree.layout(SizeProposal::exact(300.0, 80.0));
    }

    #[test]
    fn trigger_node_announces_button_role_and_haspopup() {
        let mut tree = WidgetTree::new().with_theme(bastyde_core::presets::intui::light());
        let id = tree.add(
            PopoverButton::new(Button::new(lit!("Open")))
                .content(MinSize::new(40.0, 40.0).child(RectWidget::new())),
        );
        tree.layout(SizeProposal::exact(300.0, 80.0));
        let update = tree.sync_accessibility();
        // The PopoverButton's WidgetId is the composite root; its
        // single child (the Button) carries the AT properties.
        let button_node = update
            .nodes
            .iter()
            .find(|(_, n)| n.role() == Role::Button)
            .map(|(_, n)| n)
            .expect("button node");
        assert_eq!(button_node.role(), Role::Button);
        assert_eq!(
            button_node.has_popup(),
            Some(HasPopup::Dialog),
            "default has_popup must be Dialog",
        );
        assert_eq!(
            button_node.is_expanded(),
            Some(false),
            "popover starts collapsed",
        );
        // self_id is exercised so AT scopes resolve
        let _ = widget_id_to_node_id(id);
    }

    #[test]
    fn enter_key_opens_popover_and_flips_open_signal() {
        let mut tree = WidgetTree::new().with_theme(bastyde_core::presets::intui::light());
        let pb = PopoverButton::new(Button::new(lit!("Open")))
            .content(MinSize::new(40.0, 40.0).child(RectWidget::new()));
        let open_signal = pb.open_signal();
        let id = tree.add(pb);
        tree.layout(SizeProposal::exact(300.0, 80.0));
        let button_id = tree
            .first_focusable_descendant(id)
            .expect("PopoverButton must expose a focusable inner Button");
        tree.focus(button_id);
        assert!(!open_signal.get());
        tree.dispatch_event(WidgetEvent::KeyDown {
            key: Key::Enter,
            modifiers: Modifiers::NONE,
            text: None,
        });
        tree.dispatch_event(WidgetEvent::KeyUp {
            key: Key::Enter,
            modifiers: Modifiers::NONE,
        });
        assert!(open_signal.get(), "Enter should open the popover");
    }

    #[test]
    fn show_disclosure_caret_does_not_break_pointer_clicks() {
        // Regression: when `.show_disclosure_caret(true)` is set, the
        // caret is layered on top of the Button trigger via a ZStack.
        // The caret must be pointer-pass-through so mouse clicks reach
        // the trigger; without that, the popover would only respond to
        // keyboard activation. Aim at the bottom-right quadrant of the
        // Button (where the caret paints).
        use bastyde_core::event::PointerButton;
        let mut tree = WidgetTree::new().with_theme(bastyde_core::presets::intui::light());
        let pb = PopoverButton::new(Button::new(lit!("Open")))
            .show_disclosure_caret(true)
            .content(MinSize::new(40.0, 40.0).child(RectWidget::new()));
        let open_signal = pb.open_signal();
        let id = tree.add(pb);
        tree.layout(SizeProposal::exact(300.0, 80.0));
        let trigger_id = tree
            .first_focusable_descendant(id)
            .expect("must expose a focusable inner Button");
        let trigger_bounds = tree.bounds(trigger_id);
        let caret_quadrant = bastyde_canvas::Point::new(
            trigger_bounds.x + trigger_bounds.width * 0.85,
            trigger_bounds.y + trigger_bounds.height * 0.85,
        );
        tree.pointer_down_button(caret_quadrant, PointerButton::Primary);
        tree.pointer_up_button(caret_quadrant, PointerButton::Primary);
        assert!(
            open_signal.get(),
            "click on the caret quadrant of the Button must pass through",
        );
    }
}
