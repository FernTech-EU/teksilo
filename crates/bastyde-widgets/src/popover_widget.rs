// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! `PopoverWidget<T>` — a generic trigger that opens a popover when
//! activated, plus the [`PopoverButton`] / [`PopoverIconButton`] aliases.
//!
//! Wraps a caller-built trigger (`T: PopoverTrigger`) with overlay
//! wiring: owns a `popover_open: Signal<bool>` toggled on activate /
//! dismiss, sets `has_popup` and `expanded_when` on the inner trigger so
//! AT announces the disclosure state, pre-builds the popover content as a
//! dormant subtree, and shows / hides it via [`OverlayRequest`]. The
//! `set_dormant` + `activate` + `show_overlay` sequence and the
//! dismiss-callback shape match [`DateEdit`](crate::date_edit::DateEdit)
//! so behavior across the disclosure family stays consistent.
//!
//! ```rust
//! # use bastyde_widgets::{Button, ButtonVariant, IconButton, MenuList, MenuItem, PopoverButton, PopoverIconButton};
//! # use bastyde_widgets::primitives::TextWidget;
//! # use bastyde_i18n::lit;
//! // Text trigger (HasPopup::Dialog by default, no caret):
//! let _w = PopoverButton::new(Button::new(lit!("Choose…")).variant(ButtonVariant::Plain))
//!     .content(TextWidget::new(lit!("Pick")));
//!
//! // Icon trigger (HasPopup::Menu by default, corner caret on):
//! let _w = PopoverIconButton::new(IconButton::add().toolbar())
//!     .content(MenuList::new().item(MenuItem::new(lit!("New file"))));
//! ```
//!
//! # Trigger configuration overrides
//!
//! `build()` configures the inner trigger by calling `has_popup`,
//! `expanded_when`, and `on_activate_fn` (and `share_interaction` when a
//! caret is shown). These **replace** any previous values the caller set
//! — in particular any `on_activate_fn` set before `::new` is discarded,
//! because the activate slot is owned by the popover wiring. Use
//! `on_open` / `on_close`, or observe `open_signal`, for side effects.
//!
//! # Per-trigger differences (the `PopoverTrigger` trait)
//!
//! `Button` and `IconButton` differ only in: the default `has_popup`
//! kind, whether the disclosure caret shows by default, whether the
//! caret is suppressed (IconButton at `Compact`), and how the caret's
//! color is derived. Those four points live behind `PopoverTrigger`;
//! everything else is shared by the generic.

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
use bastyde_core::styles::{PopoverStyle, PopoverStyleConfig, PopoverVariant, SharedPopoverStyle};
use bastyde_core::widget::{EventContext, LayoutContext, LayoutResponse, Widget, WidgetPlacement};
use bastyde_core::widget_id::WidgetId;
use bastyde_tokens::TextRole;

use crate::button::{Button, InteractionState, resolve_text_role};
use crate::icon_button::{
    IconButton, IconButtonSize, resolve_icon_role_embedded, resolve_icon_role_standalone,
};
use crate::popover_caret::DisclosureCaret;
use crate::primitives::ZStack;

type OnVoid = Rc<dyn Fn()>;

/// A trigger widget usable with [`PopoverWidget`]. Implemented for
/// [`Button`] and [`IconButton`]. Captures the few points where the two
/// triggers differ; everything else is handled by the generic wrapper.
pub trait PopoverTrigger: Widget + Sized + 'static {
    /// The `has_popup` kind announced by AT when the caller doesn't
    /// override it. `Button` → [`HasPopup::Dialog`]; `IconButton` →
    /// [`HasPopup::Menu`].
    fn default_has_popup() -> HasPopup;

    /// Whether the disclosure caret is painted by default. `Button` →
    /// `false` (text buttons advertise via an inline trailing chevron);
    /// `IconButton` → `true` (icon-only triggers have no label slot).
    fn default_show_caret() -> bool;

    /// Whether the caret must be suppressed for this trigger regardless
    /// of the flag (e.g. `IconButton` at `Compact` has no room).
    /// Default: never suppressed.
    fn suppress_caret(&self) -> bool {
        false
    }

    /// The `TextRole` the disclosure caret tints with, derived from the
    /// shared interaction signal so the caret and trigger tint together
    /// across hover / press / focus / disabled. Only called when a caret
    /// is shown.
    fn caret_role(&self, interaction: &Signal<InteractionState>) -> Signal<TextRole>;

    // The remaining methods delegate to inherent builder methods that
    // exist on both triggers; they're on the trait so the generic can
    // call them on a bare `T`.

    /// Share an externally-allocated interaction signal so the caret colour
    /// tracks the trigger's state (hover / press / focus / disabled) exactly.
    fn with_shared_interaction(self, signal: Signal<InteractionState>) -> Self;

    /// Annotate the trigger with the given `has_popup` kind for AT.
    fn with_has_popup(self, kind: HasPopup) -> Self;

    /// Bind the trigger's `set_expanded` disclosure state to `open`.
    fn with_expanded_when(self, open: Signal<bool>) -> Self;

    /// Install the popover's open/close handler as the trigger's activate callback.
    fn with_on_activate(self, f: impl Fn(&mut EventContext) + 'static) -> Self;

    /// Return `true` if the trigger already has an activate handler set by
    /// the caller — the wrapper replaces it and will warn at build time.
    fn has_on_activate(&self) -> bool;
}

impl PopoverTrigger for Button {
    fn default_has_popup() -> HasPopup {
        HasPopup::Dialog
    }
    fn default_show_caret() -> bool {
        false
    }
    fn caret_role(&self, interaction: &Signal<InteractionState>) -> Signal<TextRole> {
        let variant = self.current_variant();
        interaction.map(move |s| resolve_text_role(variant, *s))
    }
    fn with_shared_interaction(self, signal: Signal<InteractionState>) -> Self {
        self.share_interaction(signal)
    }
    fn with_has_popup(self, kind: HasPopup) -> Self {
        self.has_popup(kind)
    }
    fn with_expanded_when(self, open: Signal<bool>) -> Self {
        self.expanded_when(open)
    }
    fn with_on_activate(self, f: impl Fn(&mut EventContext) + 'static) -> Self {
        self.on_activate_fn(f)
    }
    fn has_on_activate(&self) -> bool {
        self.has_activate_handler()
    }
}

impl PopoverTrigger for IconButton {
    fn default_has_popup() -> HasPopup {
        HasPopup::Menu
    }
    fn default_show_caret() -> bool {
        true
    }
    fn suppress_caret(&self) -> bool {
        // Compact (22 dp) has no room for the caret without crowding the
        // icon, and Compact buttons aren't typically menu triggers.
        matches!(self.size_variant(), IconButtonSize::Compact)
    }
    fn caret_role(&self, interaction: &Signal<InteractionState>) -> Signal<TextRole> {
        if self.is_embedded() {
            interaction.map(|s| resolve_icon_role_embedded(*s))
        } else {
            interaction.map(|s| resolve_icon_role_standalone(*s))
        }
    }
    fn with_shared_interaction(self, signal: Signal<InteractionState>) -> Self {
        self.share_interaction(signal)
    }
    fn with_has_popup(self, kind: HasPopup) -> Self {
        self.has_popup(kind)
    }
    fn with_expanded_when(self, open: Signal<bool>) -> Self {
        self.expanded_when(open)
    }
    fn with_on_activate(self, f: impl Fn(&mut EventContext) + 'static) -> Self {
        self.on_activate_fn(f)
    }
    fn has_on_activate(&self) -> bool {
        self.has_activate_handler()
    }
}

/// One-shot stderr warning when a `PopoverWidget` trigger arrives with an
/// activate handler that the wrapper will overwrite. Thread-local flag
/// keeps it from repeating. (Stderr rather than `log::warn!` to avoid a
/// `log` dependency on bastyde-widgets, matching the crate convention.)
fn warn_trigger_activate_discarded() {
    thread_local! {
        static WARNED: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
    }
    WARNED.with(|w| {
        if !w.get() {
            eprintln!(
                "[bastyde-widgets::popover] PopoverWidget overwrote the trigger's \
                 on_activate_fn — the caller-set handler was discarded. Use on_open / \
                 on_close, or observe open_signal, for trigger-side side effects."
            );
            w.set(true);
        }
    });
}

/// A trigger paired with a popover surface. See the module docs for the
/// contract on which trigger properties get overridden during `build()`.
/// Use the [`PopoverButton`] / [`PopoverIconButton`] aliases for the
/// concrete trigger types.
pub struct PopoverWidget<T: PopoverTrigger> {
    trigger: Option<T>,
    content: Option<Box<dyn Widget>>,

    popover_open: Signal<bool>,
    placement: OverlayPlacement,
    dismiss_behavior: DismissBehavior,
    fade_duration: Option<Duration>,
    has_popup: HasPopup,
    show_disclosure_caret: bool,

    on_open: Option<OnVoid>,
    on_close: Option<OnVoid>,

    /// Which themed [`PopoverStyle`] surface to wrap the content in.
    /// `Some(variant)` (the default — `PopoverVariant::Default`) routes
    /// the content through the active popover style so it gets a
    /// background, border, padding, and shadow for free. `None`
    /// (`bare()`) adds the content raw — for content that is already
    /// self-chromed (a `MenuList`, which itself routes through the Menu
    /// `PopoverStyle`, or a hand-rolled surface `Panel`).
    surface_variant: Option<PopoverVariant>,
    /// Per-call style override (highest precedence over the theme slot
    /// and the built-in `RecipePopoverStyle`). Mirrors `Popover::style`.
    surface_style: Option<SharedPopoverStyle>,
    /// Accessible name for the surface's `Role::Dialog` node. Empty by
    /// default (the wrapped content usually carries its own role/name).
    surface_name: String,

    content_id: Option<WidgetId>,
    root_child_id: Option<WidgetId>,
}

/// A [`Button`] that opens a popover when activated. Alias for
/// `PopoverWidget<Button>` — `HasPopup::Dialog`, no caret by default.
pub type PopoverButton = PopoverWidget<Button>;

/// An [`IconButton`] that opens a popover when activated. Alias for
/// `PopoverWidget<IconButton>` — `HasPopup::Menu`, corner caret on by
/// default (skipped at `Compact`).
pub type PopoverIconButton = PopoverWidget<IconButton>;

impl<T: PopoverTrigger> std::fmt::Debug for PopoverWidget<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PopoverWidget")
            .field("placement", &self.placement)
            .field("dismiss_behavior", &self.dismiss_behavior)
            .field("has_popup", &self.has_popup)
            .field("show_disclosure_caret", &self.show_disclosure_caret)
            .field("popover_open", &self.popover_open.get())
            .finish_non_exhaustive()
    }
}

impl<T: PopoverTrigger> PopoverWidget<T> {
    /// Wrap a pre-configured trigger. The popover content is set
    /// separately via [`Self::content`] (required).
    pub fn new(trigger: T) -> Self {
        Self {
            trigger: Some(trigger),
            content: None,
            popover_open: Signal::new(false),
            placement: OverlayPlacement::BelowPreferred,
            dismiss_behavior: DismissBehavior::EscapeOrClickOutside,
            fade_duration: None,
            has_popup: T::default_has_popup(),
            show_disclosure_caret: T::default_show_caret(),
            on_open: None,
            on_close: None,
            surface_variant: Some(PopoverVariant::Default),
            surface_style: None,
            surface_name: String::new(),
            content_id: None,
            root_child_id: None,
        }
    }

    /// Set the popover content — added to the tree as a dormant subtree
    /// during `build()`, woken via
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
    /// no fade. See [`OverlayRequest::with_fade`] for the mechanism.
    pub fn fade_duration(mut self, d: Duration) -> Self {
        self.fade_duration = Some(d);
        self
    }

    /// Override the `has_popup` kind announced by AT. Defaults to the
    /// trigger type's [`PopoverTrigger::default_has_popup`].
    pub fn has_popup_kind(mut self, k: HasPopup) -> Self {
        self.has_popup = k;
        self
    }

    /// Whether to paint the disclosure triangle in the trigger's
    /// bottom-right corner. Defaults to the trigger type's
    /// [`PopoverTrigger::default_show_caret`]. The caret is
    /// suppressed automatically when
    /// [`PopoverTrigger::suppress_caret`] returns `true` (e.g.
    /// `IconButton` at `Compact`) regardless of this flag. AT-hidden —
    /// the popup is announced via `set_has_popup` + `set_expanded`.
    pub fn show_disclosure_caret(mut self, on: bool) -> Self {
        self.show_disclosure_caret = on;
        self
    }

    /// Notification fired on the rising edge of the popover (after the
    /// overlay show request is dispatched). No `EventContext` — observe
    /// [`Self::open_signal`] from your `build()` if you need
    /// frame / dispatch context.
    pub fn on_open(mut self, f: impl Fn() + 'static) -> Self {
        self.on_open = Some(Rc::new(f));
        self
    }

    /// Notification fired on the falling edge of the popover (when the
    /// overlay's dismiss callback runs).
    pub fn on_close(mut self, f: impl Fn() + 'static) -> Self {
        self.on_close = Some(Rc::new(f));
        self
    }

    /// Observe-only handle to the popover-open state. Apps can
    /// `ctx.effect(&pb.open_signal(), ...)` from their composite to react
    /// with full `EventContext` — `on_open` / `on_close` are
    /// notification-only (no ctx).
    pub fn open_signal(&self) -> Signal<bool> {
        self.popover_open.clone()
    }

    /// Choose which themed [`PopoverVariant`] surface wraps the content.
    /// Default is [`PopoverVariant::Default`] (elevated panel with
    /// padding + shadow). The surface is resolved from the active
    /// [`PopoverStyle`] (`theme.style_slots.popover`), so it themes
    /// app-wide.
    pub fn surface(mut self, variant: PopoverVariant) -> Self {
        self.surface_variant = Some(variant);
        self
    }

    /// Opt OUT of the themed surface: the content is added raw, with no
    /// background / border / padding. Use when the content already
    /// supplies its own chrome — a [`MenuList`](crate::MenuList) (which
    /// routes through the Menu `PopoverStyle` itself) or a hand-rolled
    /// surface `Panel`. Without this, such content would be
    /// double-chromed.
    pub fn bare(mut self) -> Self {
        self.surface_variant = None;
        self
    }

    /// Per-call [`PopoverStyle`] override for the surface (highest
    /// precedence over the theme slot and the built-in default). Mirrors
    /// [`Popover::style`](crate::Popover::style). No effect under
    /// [`bare`](Self::bare).
    pub fn surface_style(mut self, style: impl PopoverStyle) -> Self {
        self.surface_style = Some(Rc::new(style));
        self
    }

    /// Accessible name for the surface's `Role::Dialog` node. Defaults
    /// to empty (the wrapped content usually carries its own role and
    /// name). No effect under [`bare`](Self::bare) or for the Menu
    /// variant (which is presentational).
    pub fn surface_name(mut self, name: impl Into<String>) -> Self {
        self.surface_name = name.into();
        self
    }
}

impl<T: PopoverTrigger> Widget for PopoverWidget<T> {
    fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
        let content = self
            .content
            .take()
            .expect("PopoverWidget::content(...) was not set");
        // Materialize the inner content first so the surface style sees
        // a ready WidgetId (same pattern as the `Popover` widget).
        let inner_content_id = ctx.add_boxed(content);

        // Wrap the inner content in the themed popover surface
        // (background, border, padding, shadow) unless the caller opted
        // out with `bare()`. The surface is resolved per-call > theme
        // slot > built-in `RecipePopoverStyle`, so popovers theme
        // app-wide via `theme.style_slots.popover`. The id that flows
        // through the overlay machinery (dormant / gated / shown /
        // returned-as-child) is the SURFACE; focus targets the inner
        // content so it lands inside the chrome, not on the panel.
        let content_id = match self.surface_variant {
            None => inner_content_id,
            Some(variant) => {
                let style: SharedPopoverStyle = self
                    .surface_style
                    .clone()
                    .or_else(|| ctx.theme().style_slots.popover.clone())
                    .unwrap_or_else(|| Rc::new(crate::styles::RecipePopoverStyle));
                let cfg = PopoverStyleConfig {
                    content: inner_content_id,
                    variant,
                    name: self.surface_name.clone(),
                    placement: self.placement.clone(),
                    show_caret: false,
                    caret_size: 0.0,
                };
                style.make_body(&cfg, ctx)
            }
        };
        let focus_id = inner_content_id;
        ctx.set_dormant(content_id);
        // Gate the content's activation on `popover_open` so it is the single
        // source of truth. Without this, when the PopoverWidget itself is woken
        // by an ancestor's `visible_when` re-activation (e.g. a Toolbar overflow
        // chevron appearing), the activation cascade would wake the dormant
        // content in-tree — its rows would "float" outside the (closed) popover.
        // The per-pass visibility reconciliation keeps the content dormant
        // whenever the popover is closed, and `arena.activate` skips it in the
        // cascade because its gate is `false`.
        ctx.visible_when(content_id, self.popover_open.clone());
        self.content_id = Some(content_id);

        let trigger = self
            .trigger
            .take()
            .expect("PopoverWidget trigger missing (build() called twice?)");

        // The wrapper owns the trigger's activate slot (it opens the
        // popover), so any caller-set `on_activate_fn` is about to be
        // discarded. That is documented but easy to do by accident — make
        // it loud. Use `on_open` / `on_close` (or observe `open_signal`)
        // for trigger-side side effects instead.
        if trigger.has_on_activate() {
            debug_assert!(
                false,
                "PopoverWidget: the trigger's on_activate_fn is overwritten by the popover \
                 wiring and will be discarded; use on_open / on_close instead"
            );
            warn_trigger_activate_discarded();
        }

        let want_caret = self.show_disclosure_caret && !trigger.suppress_caret();

        let popover_open = self.popover_open.clone();
        let self_ref = ctx.self_id();
        let placement = self.placement.clone();
        let dismiss_behavior = self.dismiss_behavior.clone();
        let fade_duration = self.fade_duration;
        let on_open = self.on_open.clone();
        let on_close = self.on_close.clone();

        // Dismiss callback — runs when the overlay manager closes the
        // overlay (Escape, click-outside, or explicit dismiss). Flips
        // popover_open and fires the user's on_close. No `EventContext`
        // available here, so on_close is `Fn()`.
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

        // Activate handler installed onto the trigger. Toggles the
        // popover: if open, dismiss; if closed, wake the dormant content,
        // request the overlay, and move focus into it.
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
                    ctx_evt.request_focus(focus_id);
                    if let Some(cb) = on_open.as_ref() {
                        cb();
                    }
                }
            }
        };

        // With a caret, allocate the interaction signal up-front and
        // share it with the trigger so the caret's color tracks the
        // trigger's exactly. Without a caret, the trigger allocates its
        // own signal as before.
        if want_caret {
            let interaction = ctx.signal(InteractionState::Idle);
            let role_signal = trigger.caret_role(&interaction);
            let trigger = trigger
                .with_shared_interaction(interaction)
                .with_has_popup(self.has_popup)
                .with_expanded_when(popover_open.clone())
                .with_on_activate(activate);
            let trigger_id = ctx.add(trigger);
            let caret_id = ctx.add(DisclosureCaret { role: role_signal });
            let root_id = ctx.add(ZStack::new().add_child(trigger_id).add_child(caret_id));
            self.root_child_id = Some(root_id);
            // Return BOTH the trigger root AND the dormant content as
            // children so the framework links content_id under this
            // widget in the arena. Without this, content_id stays an
            // orphan root and `arena.hit_test_at` walks its subtree on
            // every click (descendants added during the content's own
            // build can re-surface as hit targets at their pre-dormant
            // positions). The layout pass skips dormant children.
            return vec![root_id, content_id];
        }

        let trigger = trigger
            .with_has_popup(self.has_popup)
            .with_expanded_when(popover_open.clone())
            .with_on_activate(activate);
        let trigger_id = ctx.add(trigger);
        self.root_child_id = Some(trigger_id);
        // See the disclosure-caret branch for the content-linking rationale.
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
        // The trigger fills our bounds; the dormant/active content never
        // participates in trigger layout — its bounds are owned by the
        // overlay manager when shown and stay at zero while dormant.
        // Dormant children are already filtered out before placements
        // reach here; if the content is active (popover open), zero its
        // placement so the parent's bounds don't clobber overlay
        // positioning.
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
        // `set_dormant` cascades correctly and `arena.hit_test_at` can
        // prune the content subtree when it's not visible.
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
        // No AT presence of our own — the inner trigger declares
        // `Role::Button`, `set_has_popup`, and `set_expanded`; the popover
        // content advertises its own role / live region. The disclosure
        // caret is decorative (set_hidden in its own accessibility()).
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::primitives::{MinSize, RectWidget};
    use bastyde_canvas::Point;
    use bastyde_core::accesskit::{HasPopup, Role};
    use bastyde_core::event::{Key, Modifiers, PointerButton, WidgetEvent};
    use bastyde_core::widget_tree::WidgetTree;
    use bastyde_i18n::lit;

    fn light_tree() -> WidgetTree {
        WidgetTree::new().with_theme(bastyde_core::presets::intui::light())
    }

    fn dummy_content() -> impl Widget {
        MinSize::new(40.0, 40.0).child(RectWidget::new())
    }

    // ── PopoverButton (text trigger) ────────────────────────────────

    #[test]
    #[should_panic(expected = "PopoverWidget::content")]
    fn button_panics_without_content() {
        let mut tree = light_tree();
        tree.add(PopoverButton::new(Button::new(lit!("Open"))));
        tree.layout(SizeProposal::exact(300.0, 80.0));
    }

    #[test]
    fn button_trigger_announces_role_and_haspopup_dialog() {
        let mut tree = light_tree();
        tree.add(PopoverButton::new(Button::new(lit!("Open"))).content(dummy_content()));
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
            Some(HasPopup::Dialog),
            "PopoverButton default has_popup must be Dialog",
        );
        assert_eq!(button_node.is_expanded(), Some(false), "starts collapsed");
    }

    #[test]
    fn button_enter_opens_popover_and_flips_open_signal() {
        let mut tree = light_tree();
        let pb = PopoverButton::new(Button::new(lit!("Open"))).content(dummy_content());
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
    fn default_wraps_content_in_themed_surface_bare_does_not() {
        // A pure-leaf content (RectWidget has no children) makes the
        // wrapping observable: with the default surface the overlay's
        // content id is the PopoverSurface (which has children); under
        // `bare()` it's the leaf itself (no children).
        fn open_overlay_content(bare: bool) -> (WidgetTree, WidgetId) {
            let mut tree = light_tree();
            let mut pb = PopoverButton::new(Button::new(lit!("Open"))).content(RectWidget::new());
            if bare {
                pb = pb.bare();
            }
            let open = pb.open_signal();
            let id = tree.add(pb);
            tree.layout(SizeProposal::exact(300.0, 120.0));
            let button = tree
                .first_focusable_descendant(id)
                .expect("focusable inner Button");
            tree.focus(button);
            tree.dispatch_event(WidgetEvent::KeyDown {
                key: Key::Enter,
                modifiers: Modifiers::NONE,
                text: None,
            });
            tree.dispatch_event(WidgetEvent::KeyUp {
                key: Key::Enter,
                modifiers: Modifiers::NONE,
            });
            assert!(open.get(), "Enter should open the popover");
            tree.layout(SizeProposal::exact(300.0, 120.0));
            let content = tree
                .overlay_manager()
                .active_content_ids()
                .first()
                .copied()
                .expect("an active overlay content");
            (tree, content)
        }

        let (tree_def, c_def) = open_overlay_content(false);
        assert!(
            !tree_def.children(c_def).is_empty(),
            "default surface should wrap the content in chrome"
        );

        let (tree_bare, c_bare) = open_overlay_content(true);
        assert!(
            tree_bare.children(c_bare).is_empty(),
            "bare() should add the leaf content raw, with no surface"
        );
    }

    #[test]
    fn button_caret_does_not_break_pointer_clicks() {
        // The disclosure caret is layered on top of the trigger in a
        // ZStack; it must be pointer-pass-through so mouse clicks reach
        // the trigger. Aim at the bottom-right quadrant where it paints.
        let mut tree = light_tree();
        let pb = PopoverButton::new(Button::new(lit!("Open")))
            .show_disclosure_caret(true)
            .content(dummy_content());
        let open_signal = pb.open_signal();
        let id = tree.add(pb);
        tree.layout(SizeProposal::exact(300.0, 80.0));
        let trigger_id = tree
            .first_focusable_descendant(id)
            .expect("must expose a focusable inner Button");
        let b = tree.bounds(trigger_id);
        let caret_quadrant = Point::new(b.x + b.width * 0.85, b.y + b.height * 0.85);
        tree.pointer_down_button(caret_quadrant, PointerButton::Primary);
        tree.pointer_up_button(caret_quadrant, PointerButton::Primary);
        assert!(
            open_signal.get(),
            "click on the caret quadrant must pass through to the trigger",
        );
    }

    // ── PopoverIconButton (icon trigger) ────────────────────────────

    #[test]
    #[should_panic(expected = "PopoverWidget::content")]
    fn icon_panics_without_content() {
        let mut tree = light_tree();
        tree.add(PopoverIconButton::new(IconButton::add()));
        tree.layout(SizeProposal::exact(300.0, 80.0));
    }

    #[test]
    fn icon_trigger_announces_haspopup_menu_collapsed() {
        let mut tree = light_tree();
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
            "PopoverIconButton default has_popup must be Menu",
        );
        assert_eq!(button_node.is_expanded(), Some(false), "starts collapsed");
    }

    #[test]
    fn icon_enter_opens_popover_and_flips_open_signal() {
        let mut tree = light_tree();
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
    fn icon_caret_false_still_focusable() {
        let mut tree = light_tree();
        let id = tree.add(
            PopoverIconButton::new(IconButton::add())
                .show_disclosure_caret(false)
                .content(dummy_content()),
        );
        tree.layout(SizeProposal::exact(300.0, 80.0));
        let _ = tree
            .first_focusable_descendant(id)
            .expect("focusable IconButton must still be present");
    }

    #[test]
    fn icon_caret_click_through_reaches_trigger() {
        let mut tree = light_tree();
        let pib = PopoverIconButton::new(IconButton::add().toolbar()).content(dummy_content());
        let open_signal = pib.open_signal();
        let id = tree.add(pib);
        tree.layout(SizeProposal::exact(300.0, 80.0));
        let trigger_id = tree
            .first_focusable_descendant(id)
            .expect("must expose a focusable IconButton");
        let b = tree.bounds(trigger_id);
        let caret_quadrant = Point::new(b.x + b.width * 0.85, b.y + b.height * 0.85);
        tree.pointer_down_button(caret_quadrant, PointerButton::Primary);
        tree.pointer_up_button(caret_quadrant, PointerButton::Primary);
        assert!(
            open_signal.get(),
            "clicking the caret quadrant of the IconButton must pass through",
        );
    }

    #[test]
    fn icon_compact_skips_caret_but_still_builds() {
        let mut tree = light_tree();
        let id = tree.add(
            PopoverIconButton::new(IconButton::add().size(IconButtonSize::Compact))
                .content(dummy_content()),
        );
        tree.layout(SizeProposal::exact(300.0, 80.0));
        let _ = tree
            .first_focusable_descendant(id)
            .expect("focusable IconButton must be present at Compact");
    }
}
