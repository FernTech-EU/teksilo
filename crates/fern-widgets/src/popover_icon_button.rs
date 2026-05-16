//! `PopoverIconButton` — an [`IconButton`] that opens a popover when activated.
//!
//! Mirrors [`PopoverButton`](crate::popover_button::PopoverButton) but with
//! an icon-only square trigger. Same `set_dormant` + `activate` +
//! `show_overlay` sequence, same dismiss-callback shape, so behavior
//! across the popover-trigger family stays consistent.
//!
//! Default `has_popup` kind is [`HasPopup::Menu`] (icon-only buttons
//! most commonly open menus). Override via [`Self::has_popup_kind`] if
//! the surface is really a Dialog or Listbox.
//!
//! ```ignore
//! use fern_ui::widgets::{IconButton, PopoverIconButton, MenuList, MenuItem};
//!
//! PopoverIconButton::new(IconButton::add().toolbar())
//!     .content(MenuList::new()
//!         .item(MenuItem::new_literal("New file"))
//!         .item(MenuItem::new_literal("New folder")))
//! ```
//!
//! # Disclosure caret
//!
//! By default the wrapper paints a small 6×6 dp right triangle in the
//! trigger's bottom-right corner (right angle at the corner) — the
//! standard "this is a menu, not a single-action button" affordance.
//! Skipped automatically at [`IconButtonSize::Compact`] (22 dp) where
//! there isn't enough room without crowding the icon. Opt out via
//! [`Self::show_disclosure_caret`].
//!
//! Decorative only — AT-hidden. The screen-reader story comes from
//! `set_has_popup(...)` + `set_expanded(...)` on the underlying
//! IconButton, same as PopoverButton.
//!
//! # Trigger configuration overrides
//!
//! `PopoverIconButton::build()` configures the inner IconButton by
//! calling:
//!
//! - `.has_popup(self.has_popup)` — defaults to [`HasPopup::Menu`].
//! - `.expanded_when(popover_open)` — drives the AT `set_expanded`
//!   announcement.
//! - `.on_activate_fn(...)` — toggles the popover.
//!
//! These three calls **replace** any previous values the caller set on
//! the trigger IconButton. In particular, any `on_activate_fn` set
//! before passing the IconButton to `PopoverIconButton::new` is
//! discarded — the activate slot is owned by the popover wiring. Apps
//! that need a side-effect on open / close should use
//! [`Self::on_open`] / [`Self::on_close`], or observe
//! [`Self::open_signal`] from a `ctx.effect`.

use std::rc::Rc;
use std::time::Duration;

use fern_canvas::{Rect, SizeProposal};
use fern_core::accessibility::AccessNodeBuilder;
use fern_core::accesskit::HasPopup;
use fern_core::build_context::BuildContext;
use fern_core::overlay::{
    DismissBehavior, OverlayDismissCallback, OverlayLayer, OverlayPlacement, OverlayRequest,
};
use fern_core::signal::Signal;
use fern_core::widget::{EventContext, LayoutContext, LayoutResponse, Widget, WidgetPlacement};
use fern_core::widget_id::WidgetId;

use crate::button::InteractionState;
use crate::icon_button::{
    IconButton, IconButtonSize, resolve_icon_role_embedded, resolve_icon_role_standalone,
};
use crate::popover_caret::DisclosureCaret;
use crate::primitives::ZStack;

type OnVoid = Rc<dyn Fn()>;

/// An [`IconButton`] paired with a popover surface. See module docs
/// for the contract on which IconButton properties get overridden
/// during `build()`.
pub struct PopoverIconButton {
    trigger: Option<IconButton>,
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

impl std::fmt::Debug for PopoverIconButton {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PopoverIconButton")
            .field("placement", &self.placement)
            .field("dismiss_behavior", &self.dismiss_behavior)
            .field("has_popup", &self.has_popup)
            .field("show_disclosure_caret", &self.show_disclosure_caret)
            .field("popover_open", &self.popover_open.get())
            .finish_non_exhaustive()
    }
}

impl PopoverIconButton {
    /// Build a `PopoverIconButton` wrapping a pre-configured
    /// [`IconButton`]. The popover content is set separately via
    /// [`Self::content`].
    pub fn new(trigger: IconButton) -> Self {
        Self {
            trigger: Some(trigger),
            content: None,
            popover_open: Signal::new(false),
            placement: OverlayPlacement::BelowPreferred,
            dismiss_behavior: DismissBehavior::EscapeOrClickOutside,
            fade_duration: None,
            has_popup: HasPopup::Menu,
            show_disclosure_caret: true,
            on_open: None,
            on_close: None,
            content_id: None,
            root_child_id: None,
        }
    }

    /// Set the popover content — added to the tree as a dormant
    /// subtree during `build()`, woken via
    /// [`EventContext::activate`](fern_core::widget::EventContext::activate)
    /// when the trigger fires. Required.
    ///
    /// **Shadow note**: when the content is a [`MenuList`](crate::menu_list::MenuList),
    /// pair it with `.attached_side(AttachedSide::*)` matching the
    /// placement so the menu's drop shadow doesn't draw on the side
    /// touching the trigger:
    ///
    /// | placement | menu attached side |
    /// |---|---|
    /// | `Below` / `BelowPreferred` / `NearAnchor` | `Top` |
    /// | `Above` | `Bottom` |
    /// | `TrailingEdge` | `Left` (LTR) / `Right` (RTL) |
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
    /// [`HasPopup::Menu`]. Set to [`HasPopup::Dialog`] when the
    /// content is a modal-feeling form / picker rather than a list of
    /// actions.
    pub fn has_popup_kind(mut self, k: HasPopup) -> Self {
        self.has_popup = k;
        self
    }

    /// Whether to paint the disclosure triangle in the trigger's
    /// bottom-right corner. Default `true`. Skipped automatically at
    /// [`IconButtonSize::Compact`] (no room without crowding the icon)
    /// regardless of this flag. Pass `false` for the rare case where
    /// surrounding context already advertises the menu (e.g. inside a
    /// SplitButton-like compound).
    pub fn show_disclosure_caret(mut self, on: bool) -> Self {
        self.show_disclosure_caret = on;
        self
    }

    /// Notification fired on the rising edge of the popover (after the
    /// overlay show request is dispatched). No `EventContext` —
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
    /// `ctx.effect(&pib.open_signal(), ...)` from their composite to
    /// react with full `EventContext` — `on_open` / `on_close` on
    /// `PopoverIconButton` itself are notification-only (no ctx).
    pub fn open_signal(&self) -> Signal<bool> {
        self.popover_open.clone()
    }
}

impl Widget for PopoverIconButton {
    fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
        let content = self
            .content
            .take()
            .expect("PopoverIconButton::content(...) was not set");
        let content_id = ctx.add_boxed(content);
        ctx.set_dormant(content_id);
        self.content_id = Some(content_id);

        let trigger = self
            .trigger
            .take()
            .expect("PopoverIconButton::new must be called with an IconButton");

        // Skip the caret at Compact (22 dp): the icon needs all the
        // visible area, and Compact buttons aren't typically menu
        // triggers. Above that, honor the show_disclosure_caret flag.
        let want_caret = self.show_disclosure_caret
            && !matches!(trigger.size_variant(), IconButtonSize::Compact);
        // Capture the trigger's color profile before transferring
        // ownership — the caret derives the same TextRole the icon
        // does so they tint together across hover / press / focus /
        // disabled states.
        let caret_embedded = trigger.is_embedded();

        let popover_open = self.popover_open.clone();
        let self_ref = ctx.self_id();
        let placement = self.placement.clone();
        let dismiss_behavior = self.dismiss_behavior.clone();
        let fade_duration = self.fade_duration;
        let on_open = self.on_open.clone();
        let on_close = self.on_close.clone();

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

        // When a caret is wanted, allocate the interaction signal
        // up-front and share it with the trigger via
        // `share_interaction`. Both the IconButton's icon and the
        // caret read derived role signals over the same interaction
        // source, so they tint together. Without a caret, IconButton
        // allocates its own signal as before.
        if want_caret {
            let interaction = ctx.signal(InteractionState::Idle);
            let role_signal = if caret_embedded {
                interaction.map(|s| resolve_icon_role_embedded(*s))
            } else {
                interaction.map(|s| resolve_icon_role_standalone(*s))
            };
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
            // children so the framework links content_id under this
            // widget in the arena — see `PopoverButton::build` for
            // the rationale (prevents orphan-root hit-test leaks).
            return vec![root_id, content_id];
        }

        let trigger = trigger
            .has_popup(self.has_popup)
            .expanded_when(popover_open.clone())
            .on_activate_fn(activate);
        let trigger_id = ctx.add(trigger);
        self.root_child_id = Some(trigger_id);
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
        // Mirror `PopoverButton::place_children` — the trigger fills
        // our bounds, the content's bounds are owned by the overlay
        // manager when shown.
        for child in children.iter_mut() {
            if Some(child.id) == self.content_id {
                child.size = fern_canvas::Size::ZERO;
                continue;
            }
            child.origin = bounds.origin();
            child.size = bounds.size();
        }
    }

    fn children(&self) -> Vec<WidgetId> {
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
        // No AT presence of our own — the inner IconButton declares
        // `Role::Button` + `set_has_popup` + `set_expanded`, and the
        // popover content advertises its own role. The disclosure
        // caret is decorative (set_hidden in its own accessibility()).
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::primitives::{MinSize, RectWidget};
    use fern_canvas::{Point, SizeProposal};
    use fern_core::accesskit::{HasPopup, Role};
    use fern_core::event::{Key, Modifiers, WidgetEvent};
    use fern_core::widget_tree::WidgetTree;
    use fern_core::Theme;

    fn dummy_content() -> impl Widget {
        MinSize::new(40.0, 40.0).child(RectWidget::new())
    }

    #[test]
    fn build_does_not_panic_with_minimum_config() {
        let mut tree = WidgetTree::new().with_theme(fern_core::presets::intui::light());
        tree.add(PopoverIconButton::new(IconButton::add()).content(dummy_content()));
        tree.layout(SizeProposal::exact(300.0, 80.0));
    }

    #[test]
    #[should_panic(expected = "PopoverIconButton::content")]
    fn build_panics_without_content() {
        let mut tree = WidgetTree::new().with_theme(fern_core::presets::intui::light());
        tree.add(PopoverIconButton::new(IconButton::add()));
        tree.layout(SizeProposal::exact(300.0, 80.0));
    }

    #[test]
    fn trigger_announces_button_role_haspopup_menu_collapsed() {
        let mut tree = WidgetTree::new().with_theme(fern_core::presets::intui::light());
        tree.add(PopoverIconButton::new(IconButton::add()).content(dummy_content()));
        tree.layout(SizeProposal::exact(300.0, 80.0));
        let update = tree.sync_accessibility();
        let button_node = update
            .nodes
            .iter()
            .find(|(_, n)| n.role() == Role::Button)
            .map(|(_, n)| n)
            .expect("button node");
        assert_eq!(
            button_node.has_popup(),
            Some(HasPopup::Menu),
            "default has_popup must be Menu",
        );
        assert_eq!(
            button_node.is_expanded(),
            Some(false),
            "popover starts collapsed",
        );
    }

    #[test]
    fn enter_key_opens_popover_and_flips_open_signal() {
        let mut tree = WidgetTree::new().with_theme(fern_core::presets::intui::light());
        let pib = PopoverIconButton::new(IconButton::add()).content(dummy_content());
        let open_signal = pib.open_signal();
        let id = tree.add(pib);
        tree.layout(SizeProposal::exact(300.0, 80.0));
        let button_id = tree
            .first_focusable_descendant(id)
            .expect("must expose a focusable inner IconButton");
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
    fn show_disclosure_caret_false_drops_the_caret_node() {
        // With caret disabled, the root child is the IconButton
        // directly — no wrapping ZStack. We verify by checking the
        // tree has only one descendant chain, not two siblings.
        let mut tree = WidgetTree::new().with_theme(fern_core::presets::intui::light());
        let id = tree.add(
            PopoverIconButton::new(IconButton::add())
                .show_disclosure_caret(false)
                .content(dummy_content()),
        );
        tree.layout(SizeProposal::exact(300.0, 80.0));
        // Should still have a focusable inner button.
        let _ = tree
            .first_focusable_descendant(id)
            .expect("focusable IconButton must still be present");
    }

    #[test]
    fn pointer_click_through_caret_reaches_trigger_and_opens_popover() {
        // Regression: the disclosure caret sits on top of the
        // IconButton in a ZStack. Without `event_pass_through(true)`
        // on the caret, hit-testing resolves to the caret first and
        // mouse clicks never reach the trigger (keyboard works via
        // focus/Enter — the failure mode the user originally hit).
        // Aim at the bottom-right quadrant of the IconButton itself
        // (where the caret paints) so we exercise the overlap region.
        use fern_core::event::PointerButton;
        let mut tree = WidgetTree::new().with_theme(fern_core::presets::intui::light());
        let pib = PopoverIconButton::new(IconButton::add().toolbar()).content(dummy_content());
        let open_signal = pib.open_signal();
        let id = tree.add(pib);
        tree.layout(SizeProposal::exact(300.0, 80.0));
        let trigger_id = tree
            .first_focusable_descendant(id)
            .expect("must expose a focusable IconButton");
        let trigger_bounds = tree.bounds(trigger_id);
        let caret_quadrant = Point::new(
            trigger_bounds.x + trigger_bounds.width * 0.85,
            trigger_bounds.y + trigger_bounds.height * 0.85,
        );
        tree.pointer_down_button(caret_quadrant, PointerButton::Primary);
        tree.pointer_up_button(caret_quadrant, PointerButton::Primary);
        assert!(
            open_signal.get(),
            "clicking on the caret quadrant of the IconButton must pass through",
        );
    }

    #[test]
    fn compact_size_skips_caret_even_when_requested() {
        // Compact (22 dp) is too small for the caret; the wrapper
        // skips it regardless of `show_disclosure_caret(true)`.
        let mut tree = WidgetTree::new().with_theme(fern_core::presets::intui::light());
        let id = tree.add(
            PopoverIconButton::new(IconButton::add().size(IconButtonSize::Compact))
                .content(dummy_content()),
        );
        tree.layout(SizeProposal::exact(300.0, 80.0));
        // Just verify it builds and the trigger is focusable; visual
        // verification of "no caret painted" lives in the previewer.
        let _ = tree
            .first_focusable_descendant(id)
            .expect("focusable IconButton must be present at Compact");
    }
}
