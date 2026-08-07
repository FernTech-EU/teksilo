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
//! # use teksilo_widgets::{Button, ButtonVariant, IconButton, MenuList, MenuItem, PopoverButton, PopoverIconButton};
//! # use teksilo_widgets::primitives::TextWidget;
//! # use teksilo_i18n::lit;
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

use teksilo_canvas::{Rect, SizeProposal};
use teksilo_core::accessibility::AccessNodeBuilder;
use teksilo_core::accesskit::HasPopup;
use teksilo_core::build_context::BuildContext;
use teksilo_core::overlay::{
    DismissBehavior, OverlayDismissCallback, OverlayLayer, OverlayPlacement, OverlayRequest,
};
use teksilo_core::signal::Signal;
use teksilo_core::styles::{PopoverStyle, PopoverStyleConfig, PopoverVariant, SharedPopoverStyle};
use teksilo_core::widget::{EventContext, LayoutContext, LayoutResponse, Widget, WidgetPlacement};
use teksilo_core::widget_id::WidgetId;
use teksilo_tokens::TextRole;

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
/// `log` dependency on teksilo-widgets, matching the crate convention.)
fn warn_trigger_activate_discarded() {
    thread_local! {
        static WARNED: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
    }
    WARNED.with(|w| {
        if !w.get() {
            eprintln!(
                "[teksilo-widgets::popover] PopoverWidget overwrote the trigger's \
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
    /// Name of the global action that toggles this popover, if the caller asked
    /// for one. See [`PopoverWidget::open_action`].
    open_action: Option<&'static str>,
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

    /// Optional plain tooltip text shown after a hover delay. Mutually exclusive
    /// with the rich / composite slots — every setter clears the other two so
    /// the last call wins.
    tooltip_text: Option<teksilo_i18n::LocalizedString>,
    /// Optional rich tooltip source (registry key or inline content).
    rich_tooltip_source: Option<crate::tooltip::RichTooltipSource>,
    /// Optional composite tooltip body (arbitrary widget tree).
    composite_tooltip_content: Option<Box<dyn Widget>>,
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
            open_action: None,
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
            tooltip_text: None,
            rich_tooltip_source: None,
            composite_tooltip_content: None,
        }
    }

    /// Set the popover content — added to the tree as a dormant subtree
    /// during `build()`, woken via
    /// [`EventContext::activate`](teksilo_core::widget::EventContext::activate)
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

    /// Observe-only handle to the popover-open state.
    ///
    /// **Read-back only — writing this does not open the popover.** Presenting
    /// an overlay needs an `EventContext` (`show_overlay` + `request_focus`),
    /// which no signal observer has; this field is the mirror the trigger writes
    /// after it has done that work. To open the popover from somewhere other
    /// than its trigger, use [`open_action`](Self::open_action).
    pub fn open_signal(&self) -> Signal<bool> {
        self.popover_open.clone()
    }

    /// Register a **named global action** that toggles this popover, so a menu
    /// entry, a global shortcut or `ctx.send_intent(...)` can open it — not only
    /// a click on its own trigger.
    ///
    /// Without this a popover is reachable by pointer alone. `on_open` /
    /// `on_close` are notification-only and `open_signal` is a read-back mirror
    /// (see its doc), so an app that wanted "Go to… ⌘G" next to its button had
    /// no way to wire the second half. Action handlers are the one place that
    /// *does* get an `EventContext`, which is exactly what presenting an overlay
    /// requires — so the action runs the identical toggle the trigger runs, and
    /// the two can never drift.
    ///
    /// Registered with `register_action_global`, deliberately: intents walk
    /// source-widget → root, and a menu renders in an **overlay** that is a
    /// sibling of the popover's own subtree, so a plain `register_action` would
    /// never be reached from a menu item. Pair it with
    /// `register_shortcut_global` in the app for the keystroke.
    ///
    /// ```ignore
    /// PopoverButton::new(Button::new(tr!(go_to())))
    ///     .content(palette)
    ///     .open_action("go.to")
    /// // elsewhere: MenuEntry::new(tr!(go_to())).intent("go.to").shortcut("go.to")
    /// ```
    pub fn open_action(mut self, intent: &'static str) -> Self {
        self.open_action = Some(intent);
        self
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

    /// Show a plain single-line tooltip on the trigger after a hover delay.
    /// Mutually exclusive with [`rich_tooltip`](Self::rich_tooltip),
    /// [`rich_tooltip_content`](Self::rich_tooltip_content), and
    /// [`composite_tooltip`](Self::composite_tooltip) — each setter clears
    /// the other three so the last call wins. The tooltip anchors on the
    /// trigger, not on the popover content.
    pub fn tooltip(mut self, text: impl Into<teksilo_i18n::LocalizedString>) -> Self {
        self.tooltip_text = Some(text.into());
        self.rich_tooltip_source = None;
        self.composite_tooltip_content = None;
        self
    }

    /// Show a rich tooltip (looked up by registry key) on the trigger after
    /// a hover delay. Mutually exclusive with the other tooltip setters —
    /// the last call wins.
    pub fn rich_tooltip(mut self, key: impl Into<String>) -> Self {
        self.rich_tooltip_source = Some(crate::tooltip::RichTooltipSource::Key(key.into()));
        self.tooltip_text = None;
        self.composite_tooltip_content = None;
        self
    }

    /// Show an inline rich tooltip (pre-built [`TooltipContent`]) on the
    /// trigger after a hover delay. Mutually exclusive with the other tooltip
    /// setters — the last call wins.
    ///
    /// [`TooltipContent`]: crate::tooltip::TooltipContent
    pub fn rich_tooltip_content(mut self, content: crate::tooltip::TooltipContent) -> Self {
        self.rich_tooltip_source = Some(crate::tooltip::RichTooltipSource::Content(content));
        self.tooltip_text = None;
        self.composite_tooltip_content = None;
        self
    }

    /// Show a composite tooltip (arbitrary widget tree) on the trigger after
    /// a longer hover delay. Mutually exclusive with the other tooltip setters
    /// — the last call wins.
    pub fn composite_tooltip(mut self, content: impl Widget + 'static) -> Self {
        self.composite_tooltip_content = Some(Box::new(content));
        self.tooltip_text = None;
        self.rich_tooltip_source = None;
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
                    .unwrap_or_else(|| Rc::new(crate::styles::RecipePopoverStyle::default()));
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
        //
        // Built as an `Rc` so `open_action` can register the *same* closure as a
        // named global action. Sharing it (rather than writing a second, similar
        // one) is the point: a menu entry and the trigger must not be able to
        // disagree about what opening this popover means.
        let activate: Rc<dyn Fn(&mut EventContext)> = Rc::new({
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
        });

        // The named-action door. Registered global, not local: intents walk
        // source-widget → root, and a menu renders in an overlay that is a
        // sibling of this widget's subtree, so a plain `register_action` would
        // never be reached from a menu item.
        if let Some(intent) = self.open_action {
            let act = activate.clone();
            ctx.register_action_global(
                teksilo_core::action::Action::new(intent)
                    .on_invoke(move |_intent, ctx_evt| act(ctx_evt)),
            );
        }

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
                .with_on_activate({
                    let act = activate.clone();
                    move |c: &mut EventContext| act(c)
                });
            let trigger_id = ctx.add(trigger);
            let caret_id = ctx.add(DisclosureCaret { role: role_signal });
            let root_id = ctx.add(ZStack::new().add_child(trigger_id).add_child(caret_id));
            self.root_child_id = Some(root_id);
            if let Some(content) = self.composite_tooltip_content.take() {
                let delay = ctx.theme().motion.tooltip_delay_heavy;
                crate::tooltip::attach_composite_tooltip_boxed(ctx, root_id, content, delay);
            } else if let Some(source) = self.rich_tooltip_source.clone() {
                let delay = ctx.theme().motion.tooltip_delay;
                crate::tooltip::attach_rich_tooltip_source(ctx, root_id, source, delay);
            } else if let Some(text) = self.tooltip_text.clone() {
                let tooltip_widget = crate::tooltip::TooltipWidget::new(text);
                let tooltip_id = ctx.add(tooltip_widget);
                let delay = ctx.theme().motion.tooltip_delay;
                ctx.attach_tooltip(root_id, tooltip_id, delay);
            }
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
            .with_on_activate(move |c: &mut EventContext| activate(c));
        let trigger_id = ctx.add(trigger);
        self.root_child_id = Some(trigger_id);
        if let Some(content) = self.composite_tooltip_content.take() {
            let delay = ctx.theme().motion.tooltip_delay_heavy;
            crate::tooltip::attach_composite_tooltip_boxed(ctx, trigger_id, content, delay);
        } else if let Some(source) = self.rich_tooltip_source.clone() {
            let delay = ctx.theme().motion.tooltip_delay;
            crate::tooltip::attach_rich_tooltip_source(ctx, trigger_id, source, delay);
        } else if let Some(text) = self.tooltip_text.clone() {
            let tooltip_widget = crate::tooltip::TooltipWidget::new(text);
            let tooltip_id = ctx.add(tooltip_widget);
            let delay = ctx.theme().motion.tooltip_delay;
            ctx.attach_tooltip(trigger_id, tooltip_id, delay);
        }
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
                child.size = teksilo_canvas::Size::ZERO;
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
    use teksilo_canvas::Point;
    use teksilo_core::accesskit::{HasPopup, Role};
    use teksilo_core::event::{Key, Modifiers, PointerButton, WidgetEvent};
    use teksilo_core::widget_tree::WidgetTree;
    use teksilo_i18n::lit;

    fn light_tree() -> WidgetTree {
        WidgetTree::new().with_theme(teksilo_core::presets::intui::light())
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

    /// `open_action` opens the popover from a **sibling** widget's intent.
    ///
    /// The sibling placement is the test, not incidental scenery: intents walk
    /// source-widget → root, so a locally-registered action would never be
    /// reached from a menu — which renders in an overlay that is a sibling of
    /// the popover's subtree, exactly like this button. Firing from a child of
    /// the popover would pass with either registration and prove nothing.
    #[test]
    fn open_action_opens_the_popover_from_a_sibling_intent() {
        use crate::primitives::VStack;
        use teksilo_core::intent::Intent;

        let mut tree = light_tree();
        let pb = PopoverButton::new(Button::new(lit!("Open")))
            .content(dummy_content())
            .open_action("test.open");
        let open_signal = pb.open_signal();
        let pb_id = tree.add(pb);
        let fire_id = tree.add(
            Button::new(lit!("Fire"))
                .on_activate_fn(|ctx| ctx.send_intent(Intent::new("test.open"))),
        );
        tree.add(VStack::new().add_child(pb_id).add_child(fire_id));
        tree.layout(SizeProposal::exact(300.0, 160.0));

        assert!(!open_signal.get(), "starts closed");

        let fire_btn = tree.first_focusable_descendant(fire_id).unwrap_or(fire_id);
        tree.focus(fire_btn);
        tree.dispatch_event(WidgetEvent::KeyDown {
            key: Key::Enter,
            modifiers: Modifiers::NONE,
            text: None,
        });
        tree.dispatch_event(WidgetEvent::KeyUp {
            key: Key::Enter,
            modifiers: Modifiers::NONE,
        });
        assert!(
            open_signal.get(),
            "the named action must open the popover from off its own subtree"
        );
    }

    /// The action *toggles*, sharing one closure with the trigger — so a menu
    /// entry and a click can never disagree about what the popover does.
    ///
    /// Fired from **inside** the panel, which is the only place the toggle's
    /// close branch is still reachable. A sibling cannot reach it: taking focus
    /// away from an open popover now dismisses it (non-modal overlays follow
    /// focus out rather than trapping it), so by the time an outside control is
    /// focused enough to be activated, there is nothing left to close and the
    /// shared closure correctly takes its *open* branch. That is not new
    /// asymmetry — the popover's default `EscapeOrClickOutside` already meant a
    /// real pointer click on that sibling dismissed it before activating. The
    /// keyboard simply stopped disagreeing with the mouse.
    /// `open_action_opens_the_popover_from_a_sibling_intent` above still pins
    /// the global-registration half.
    #[test]
    fn open_action_toggles_rather_than_only_opening() {
        use teksilo_core::intent::Intent;

        let mut tree = light_tree();
        let pb = PopoverButton::new(Button::new(lit!("Open")))
            .content(
                Button::new(lit!("Fire"))
                    .on_activate_fn(|ctx| ctx.send_intent(Intent::new("test.toggle"))),
            )
            .open_action("test.toggle");
        let open_signal = pb.open_signal();
        let pb_id = tree.add(pb);
        tree.layout(SizeProposal::exact(300.0, 160.0));

        let trigger = tree
            .first_focusable_descendant(pb_id)
            .expect("the trigger is the only focusable while closed");
        tree.focus(trigger);
        let enter = |tree: &mut WidgetTree| {
            tree.dispatch_event(WidgetEvent::KeyDown {
                key: Key::Enter,
                modifiers: Modifiers::NONE,
                text: None,
            });
            tree.dispatch_event(WidgetEvent::KeyUp {
                key: Key::Enter,
                modifiers: Modifiers::NONE,
            });
        };

        enter(&mut tree);
        assert!(open_signal.get(), "first fire opens");
        // Opening moved focus into the panel, onto its own Fire button — so the
        // next Enter runs the same shared closure without focus ever leaving.
        enter(&mut tree);
        assert!(!open_signal.get(), "second fire closes");
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

    #[test]
    fn tooltip_appears_on_hover() {
        let mut tree = light_tree();
        let id = tree.add(
            PopoverButton::new(Button::new(lit!("Open")))
                .content(dummy_content())
                .tooltip(lit!("Tip")),
        );
        tree.layout(SizeProposal::exact(300.0, 80.0));
        tree.pointer_move(tree.bounds(id).center());
        tree.advance_time(std::time::Duration::from_secs(1));
        assert_eq!(
            tree.active_overlays().len(),
            1,
            "tooltip should appear on hover"
        );
        assert!(tree.find_by_label("Tip").is_some());
    }

    #[derive(Debug)]
    struct FocusableLeaf;
    impl Widget for FocusableLeaf {
        fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
            ctx.apply_self_handlers(
                teksilo_core::widget_builder::HandlerSet::new().focusable(true),
            );
            vec![]
        }
        fn layout_response(
            &self,
            proposal: SizeProposal,
            _ctx: &LayoutContext,
        ) -> teksilo_core::widget::LayoutResponse {
            proposal.resolve(12.0, 12.0).into()
        }
    }

    /// Open a popover, Tab past its last control, and it must go.
    ///
    /// A popover implements the Disclosure pattern, which mandates no focus
    /// containment — so Tab genuinely leaves. What must *not* survive that is
    /// the panel itself: an open popover with the focus ring somewhere behind
    /// it fails WCAG 2.2 SC 2.4.11 (Focus Not Obscured). Note the content's
    /// natural Tab slot is already correct — it is built as a child of the
    /// trigger, so it follows the trigger the way a disclosure's panel follows
    /// its button. Only the dismissal was missing.
    #[test]
    fn tab_out_of_popover_dismisses_it() {
        let mut tree = light_tree();
        let pb = PopoverButton::new(Button::new(lit!("Open"))).content(
            crate::primitives::VStack::new()
                .child(FocusableLeaf)
                .child(FocusableLeaf),
        );
        let open_signal = pb.open_signal();
        let id = tree.add(pb);
        let after = tree.add(FocusableLeaf);
        tree.layout(SizeProposal::exact(300.0, 400.0));
        let button_id = tree.first_focusable_descendant(id).expect("inner Button");

        tree.focus(button_id);
        tree.dispatch_event(WidgetEvent::KeyDown {
            key: Key::Enter,
            modifiers: Modifiers::NONE,
            text: None,
        });
        tree.dispatch_event(WidgetEvent::KeyUp {
            key: Key::Enter,
            modifiers: Modifiers::NONE,
        });
        assert!(open_signal.get(), "precondition: Enter opens the popover");
        assert_eq!(tree.active_overlays().len(), 1);

        // Tab within the content — two focusables, so the first Tab stays inside
        // and must NOT dismiss anything.
        tree.press_key(Key::Tab, Modifiers::NONE);
        assert_eq!(
            tree.active_overlays().len(),
            1,
            "moving between the popover's own controls is not leaving it"
        );

        // The next Tab leaves the content for good.
        tree.press_key(Key::Tab, Modifiers::NONE);
        assert_eq!(tree.focused(), Some(after), "focus lands past the trigger");
        assert!(
            tree.active_overlays().is_empty(),
            "the popover must not stay open behind the focus ring"
        );
        assert!(!open_signal.get(), "and its open signal must follow");
    }

    /// Shift+Tab off the front of the content leaves it just as surely — and
    /// lands on the trigger, which is where Escape would have left it.
    #[test]
    fn shift_tab_off_the_front_of_a_popover_dismisses_it() {
        let mut tree = light_tree();
        let pb = PopoverButton::new(Button::new(lit!("Open"))).content(
            crate::primitives::VStack::new()
                .child(FocusableLeaf)
                .child(FocusableLeaf),
        );
        let open_signal = pb.open_signal();
        let id = tree.add(pb);
        tree.add(FocusableLeaf);
        tree.layout(SizeProposal::exact(300.0, 400.0));
        let button_id = tree.first_focusable_descendant(id).expect("inner Button");

        tree.focus(button_id);
        tree.dispatch_event(WidgetEvent::KeyDown {
            key: Key::Enter,
            modifiers: Modifiers::NONE,
            text: None,
        });
        tree.dispatch_event(WidgetEvent::KeyUp {
            key: Key::Enter,
            modifiers: Modifiers::NONE,
        });
        assert!(open_signal.get());

        tree.press_key(Key::Tab, Modifiers::SHIFT);
        assert_eq!(tree.focused(), Some(button_id), "back onto the trigger");
        assert!(
            tree.active_overlays().is_empty(),
            "leaving through the front dismisses it too"
        );
    }
}
