// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Link — a clickable text label rendered as underlined inline text.
//!
//! `Link` is Teksilo's hyperlink control: it responds to tap, Enter, and
//! Space like a `Button`, but renders as styled underlined text rather than a
//! bordered box. It supports an optional `url` field (informational — the app
//! decides whether and how to open it), a reactive `visited` state that shifts
//! the text colour, and all three tooltip tiers (plain / rich / composite).
//!
//! Keyboard behaviour follows the platform link convention: Space and Enter
//! activate; a bare KeyUp with no preceding KeyDown is ignored (lone-KeyUp
//! guard). The focus ring appears only after keyboard navigation
//! (`focus_visible`), not after a mouse click.
//!
//! ## Accessibility
//!
//! `Role::Link` with the label as the AT name. When `url` is set it is
//! forwarded to `set_url` so screen readers can announce the destination.
//! Exposes `Action::Click` and `Action::Focus`.
//!
//! ```rust
//! # use teksilo_widgets::Link;
//! # use teksilo_i18n::lit;
//! let _w = Link::new(lit!("Open documentation"))
//!     .url("https://example.com/docs");
//! ```
//!
//! ## Touch and pen
//!
//! The pressed state is the framework's (`docs/touch-and-pen.md` §7.1), so it
//! survives a slide-off and comes back on re-entry, and a pan claimant winning
//! the press clears it with no release. Following the link lands on the
//! release, as it always did.
//!
//! A link is text-height, so it can fall under the 24 dp target floor; it is
//! reached by the miss-only slop pass, which re-attributes a coarse near miss
//! to it whenever nothing nearer takes presses. WCAG 2.2 SC 2.5.8's *inline*
//! exception covers a link whose size is constrained by the line height of the
//! text around it.

use std::cell::Cell;
use std::rc::Rc;

use teksilo_canvas::{Rect, Size, SizeProposal};
use teksilo_core::accessibility::AccessNodeBuilder;
use teksilo_core::build_context::BuildContext;
use teksilo_core::event::{EventResponse, Key, WidgetEvent};
use teksilo_core::signal::{Prop, Signal};
use teksilo_core::styles::{LinkStyleConfig, SharedLinkStyle};
use teksilo_core::widget::{CursorIcon, EventContext, LayoutContext, Widget, WidgetPlacement};
use teksilo_core::widget_builder::HandlerSet;
use teksilo_core::widget_id::WidgetId;

use crate::button::InteractionState;
use teksilo_i18n::LocalizedString;

type CommandFactory = Box<dyn Fn(&mut EventContext)>;

/// A clickable text link that renders as underlined inline text.
pub struct Link {
    text: LocalizedString,
    url: Option<String>,
    action: Option<CommandFactory>,
    tooltip_text: Option<LocalizedString>,
    rich_tooltip_source: Option<crate::tooltip::RichTooltipSource>,
    composite_tooltip_content: Option<Box<dyn teksilo_core::widget::Widget>>,
    interaction: Option<Signal<InteractionState>>,
    /// Visited state — orthogonal to `InteractionState`. The app owns
    /// the URL-visit tracking; this signal toggles `TextRole::LinkVisited`
    /// when no transient interaction (hover / press) is active.
    /// Default is a permanently-`false` signal so links that don't
    /// represent URLs render as unvisited.
    visited: Option<Prop<bool>>,
    /// Enabled state, static or reactive; forwarded to the arena at
    /// build time.
    enabled: Prop<bool>,
    /// Per-call override for the link chrome.
    style_override: Option<SharedLinkStyle>,
    root_child_id: Option<WidgetId>,
}

impl Link {
    /// Create a link with the given display text.
    pub fn new(text: impl Into<LocalizedString>) -> Self {
        let ls: LocalizedString = text.into();
        Self {
            text: ls,
            url: None,
            action: None,
            tooltip_text: None,
            rich_tooltip_source: None,
            composite_tooltip_content: None,
            interaction: None,
            visited: None,
            enabled: Prop::Static(true),
            style_override: None,
            root_child_id: None,
        }
    }

    /// Mark the link's target as visited. Drives `TextRole::LinkVisited`
    /// when no transient interaction (hover / press) is active. Visited
    /// is overridden by hover/press, following the web convention. The
    /// app owns the signal (typically backed by URL-history state).
    pub fn visited(mut self, visited: impl Into<Prop<bool>>) -> Self {
        self.visited = Some(visited.into());
        self
    }

    /// Per-call style override for the link chrome.
    pub fn style(mut self, style: impl teksilo_core::styles::LinkStyle) -> Self {
        self.style_override = Some(Rc::new(style));
        self
    }

    /// Closure invoked on activation.
    pub fn on_activate_fn(mut self, f: impl Fn(&mut EventContext) + 'static) -> Self {
        self.action = Some(Box::new(f));
        self
    }

    /// Set a URL for the link (informational — not automatically opened).
    pub fn url(mut self, url: impl Into<String>) -> Self {
        self.url = Some(url.into());
        self
    }

    /// Attach a plain single-line tooltip shown after a hover delay.
    /// Mutually exclusive with `rich_tooltip` / `composite_tooltip` — last call wins.
    pub fn tooltip(mut self, text: impl Into<LocalizedString>) -> Self {
        self.tooltip_text = Some(text.into());
        self.rich_tooltip_source = None;
        self.composite_tooltip_content = None;
        self
    }

    /// Attach a rich tooltip resolved from the app-wide tooltip
    /// registry. See [`Button::rich_tooltip`](crate::button::Button::rich_tooltip).
    pub fn rich_tooltip(mut self, key: impl Into<String>) -> Self {
        self.rich_tooltip_source = Some(crate::tooltip::RichTooltipSource::Key(key.into()));
        self.tooltip_text = None;
        self.composite_tooltip_content = None;
        self
    }

    /// Attach a rich tooltip driven by inline `TooltipContent`.
    pub fn rich_tooltip_content(mut self, content: crate::tooltip::TooltipContent) -> Self {
        self.rich_tooltip_source = Some(crate::tooltip::RichTooltipSource::Content(content));
        self.tooltip_text = None;
        self.composite_tooltip_content = None;
        self
    }

    /// Attach a composite tooltip — third tier, hosting an arbitrary
    /// widget tree. See [`Button::composite_tooltip`](crate::button::Button::composite_tooltip).
    pub fn composite_tooltip(
        mut self,
        content: impl teksilo_core::widget::Widget + 'static,
    ) -> Self {
        self.composite_tooltip_content = Some(Box::new(content));
        self.tooltip_text = None;
        self.rich_tooltip_source = None;
        self
    }

    /// Return the URL previously set via [`url`](Self::url), if any.
    pub fn get_url(&self) -> Option<&str> {
        self.url.as_deref()
    }

    /// Set the enabled state, statically or reactively. Forwarded to the
    /// arena at build time — a bound `Signal<bool>` updates live as it
    /// changes.
    pub fn enabled(mut self, enabled: impl Into<Prop<bool>>) -> Self {
        self.enabled = enabled.into();
        self
    }
}

impl std::fmt::Debug for Link {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Link").field("text", &self.text).finish()
    }
}

impl Widget for Link {
    fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
        let self_id = ctx.self_id();
        // Forward the enabled state into the arena; see IconButton.
        ctx.enabled_when(self_id, self.enabled.clone());
        let effective_enabled = ctx.effective_enabled_signal(self_id);

        let interaction = ctx.signal(InteractionState::Idle);
        self.interaction = Some(interaction.clone());

        // Derive the four state bools `LinkStyle` expects from the
        // single `InteractionState` signal. `is_disabled` derives
        // from the arena (reactive) instead of a build-time snapshot.
        let is_hovered = interaction.map(|s| matches!(s, InteractionState::Hovered));
        let is_pressed = interaction.map(|s| matches!(s, InteractionState::Pressed));
        // `:focus-visible`: reveal the focus ring during keyboard navigation
        // only, not on a mouse click. Gate raw focus on the input-modality
        // signal (true after a key event, false after pointer-down).
        let is_focused = interaction
            .map(|s| matches!(s, InteractionState::Focused))
            .and(&ctx.focus_visible());
        let is_visited = self
            .visited
            .as_ref()
            .map(|p| p.as_signal())
            .unwrap_or_else(|| Signal::new(false));
        let is_disabled = effective_enabled.map(|on| !*on);

        let style: SharedLinkStyle = self
            .style_override
            .clone()
            .or_else(|| ctx.theme().style_slots.link.clone())
            .unwrap_or_else(|| {
                Rc::new(crate::styles::RecipeLinkStyle::for_tokens(
                    &ctx.theme().input,
                ))
            });
        let root_id = style.make_body(
            &LinkStyleConfig {
                text: self.text.clone().into(),
                is_hovered,
                is_pressed,
                is_focused,
                is_visited,
                is_disabled,
            },
            ctx,
        );

        if let Some(content) = self.composite_tooltip_content.take() {
            let delay = ctx.theme().motion.tooltip_delay_heavy;
            crate::tooltip::attach_composite_tooltip_boxed(ctx, root_id, content, delay);
        } else if let Some(source) = self.rich_tooltip_source.take() {
            let delay = ctx.theme().motion.tooltip_delay;
            crate::tooltip::attach_rich_tooltip_source(ctx, root_id, source, delay);
        } else if let Some(tooltip_text) = self.tooltip_text.clone() {
            let delay = ctx.theme().motion.tooltip_delay;
            crate::tooltip::attach_plain_tooltip(ctx, root_id, tooltip_text, delay);
        }

        self.root_child_id = Some(root_id);

        // --- V2 attached handlers ---
        let action = self.action.take();
        let action_rc: std::rc::Rc<Option<CommandFactory>> = std::rc::Rc::new(action);
        let action_for_tap = action_rc.clone();
        let action_for_key = action_rc.clone();
        let action_for_access = action_rc.clone();
        let int_tap = interaction.clone();
        let int_hover = interaction.clone();
        let int_key = interaction.clone();
        let int_focus = interaction.clone();

        // The pointer press is the framework's, not this control's own: the
        // router knows about a press that slid off its target, one that slid
        // back on, and one a pan claimant took away with no release to reset
        // from — none of which a `PointerDown` / `PointerUp` pair here can
        // see. `docs/touch-and-pen.md` §7.1. `pointer_over` carries the hover
        // truth across the press, so a press that ends without an activation
        // rests on the right state.
        let pointer_over = Rc::new(Cell::new(false));
        crate::button::bind_press_interaction(ctx, interaction.clone(), pointer_over.clone());

        let handler_set = HandlerSet::new()
            .on_tap({
                let hovering = pointer_over.clone();
                move |_pos, ctx: &mut EventContext| {
                    if let Some(ref action) = *action_for_tap {
                        action(ctx);
                    }
                    int_tap.set(if ctx.pointer_kind().hovers() {
                        hovering.set(true);
                        InteractionState::Hovered
                    } else {
                        InteractionState::Idle
                    });
                }
            })
            .on_hover({
                let hovering = pointer_over.clone();
                move |entered: bool, _ctx: &mut EventContext| {
                    hovering.set(entered);
                    if entered {
                        int_hover.set(InteractionState::Hovered);
                    } else {
                        int_hover.set(InteractionState::Idle);
                    }
                }
            })
            .on_key({
                move |event: &WidgetEvent, ctx: &mut EventContext| -> EventResponse {
                    match event {
                        WidgetEvent::KeyDown {
                            key: Key::Space | Key::Enter,
                            ..
                        } => {
                            int_key.set(InteractionState::Pressed);
                            EventResponse::Handled
                        }
                        WidgetEvent::KeyUp {
                            key: Key::Space | Key::Enter,
                            ..
                        } => {
                            // Lone-KeyUp guard: only activate if we saw the
                            // matching KeyDown (state is Pressed). A KeyUp with
                            // no preceding KeyDown — e.g. a shortcut consumed the
                            // KeyDown and focus returned here — must NOT activate.
                            if int_key.get() != InteractionState::Pressed {
                                return EventResponse::Ignored;
                            }
                            if let Some(ref action) = *action_for_key {
                                action(ctx);
                            }
                            int_key.set(InteractionState::Focused);
                            EventResponse::Handled
                        }
                        _ => EventResponse::Ignored,
                    }
                }
            })
            .on_focus({
                move |gained: bool, _ctx: &mut EventContext| {
                    if gained {
                        if int_focus.get() == InteractionState::Idle {
                            int_focus.set(InteractionState::Focused);
                        }
                    } else {
                        int_focus.set(InteractionState::Idle);
                    }
                }
            })
            .on_access_action({
                move |action: teksilo_core::accesskit::Action,
                      ctx: &mut EventContext|
                      -> EventResponse {
                    if action == teksilo_core::accesskit::Action::Click {
                        if let Some(ref act) = *action_for_access {
                            act(ctx);
                        }
                        EventResponse::Handled
                    } else {
                        EventResponse::Ignored
                    }
                }
            })
            // Focus walker skips disabled subtrees; cursor stays
            // Pointer here and the framework can choose to override
            // for disabled subtrees in a future change.
            .focusable(true)
            .cursor(CursorIcon::Pointer);

        ctx.apply_self_handlers(handler_set);

        vec![root_id]
    }

    fn layout_response(
        &self,
        proposal: SizeProposal,
        ctx: &LayoutContext,
    ) -> teksilo_core::widget::LayoutResponse {
        if let Some(root) = self.root_child_id
            && let Some(size) = ctx.child_size(root, proposal)
        {
            return (size).into();
        }
        proposal.resolve(0.0, 0.0).into()
    }

    fn place_children(
        &self,
        bounds: Rect,
        _proposal: SizeProposal,
        children: &mut [WidgetPlacement],
        _ctx: &LayoutContext,
    ) {
        for child in children.iter_mut() {
            child.origin = teksilo_canvas::Point::new(bounds.x, bounds.y);
            child.size = Size::new(bounds.width, bounds.height);
        }
    }

    fn accessibility(&self, builder: &mut AccessNodeBuilder) {
        builder.set_role(teksilo_core::accesskit::Role::Link);
        builder.set_name(self.text.resolve_now());
        if let Some(ref url) = self.url {
            builder.set_url(url.clone());
        }
        // Framework a11y walker sets `set_disabled` from arena state.
        // Actions are always advertised — when disabled the framework
        // gates them at dispatch via `arena.is_enabled`.
        builder.add_action(teksilo_core::accesskit::Action::Click);
        builder.add_action(teksilo_core::accesskit::Action::Focus);
    }

    fn children(&self) -> Vec<WidgetId> {
        self.root_child_id.into_iter().collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;
    use teksilo_core::event::Modifiers;
    use teksilo_core::widget_tree::WidgetTree;
    use teksilo_i18n::lit;

    #[test]
    fn keyup_without_keydown_does_not_fire() {
        // Lone-KeyUp guard: when a shortcut consumes the KeyDown and
        // focus returns to the link, the trailing KeyUp must NOT activate.
        let mut tree = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
        let fired = Rc::new(Cell::new(0_u32));
        let fired_for_link = fired.clone();
        let link = tree.add(Link::new(lit!("T")).on_activate_fn(move |_ctx| {
            fired_for_link.set(fired_for_link.get() + 1);
        }));
        tree.layout(SizeProposal::exact(200.0, 80.0));
        tree.focus(link);

        tree.dispatch_event(WidgetEvent::KeyUp {
            key: Key::Enter,
            modifiers: Modifiers::NONE,
        });
        assert_eq!(
            fired.get(),
            0,
            "a lone KeyUp (no matching KeyDown) must not activate the link",
        );

        tree.dispatch_event(WidgetEvent::KeyDown {
            key: Key::Enter,
            modifiers: Modifiers::NONE,
            text: None,
        });
        tree.dispatch_event(WidgetEvent::KeyUp {
            key: Key::Enter,
            modifiers: Modifiers::NONE,
        });
        assert_eq!(
            fired.get(),
            1,
            "a matched KeyDown + KeyUp pair must activate exactly once",
        );
    }

    // -----------------------------------------------------------------
    // The framework press (docs/touch-and-pen.md §7.1)
    // -----------------------------------------------------------------

    struct PressProbe(std::rc::Rc<std::cell::RefCell<Option<(Signal<bool>, Signal<bool>)>>>);

    impl teksilo_core::styles::LinkStyle for PressProbe {
        fn make_body(
            &self,
            cfg: &teksilo_core::styles::LinkStyleConfig,
            ctx: &mut BuildContext,
        ) -> WidgetId {
            *self.0.borrow_mut() = Some((cfg.is_pressed.clone(), cfg.is_hovered.clone()));
            ctx.add(crate::primitives::FixedSize::new().width(60.0).height(18.0))
        }
    }

    #[allow(clippy::type_complexity)]
    fn probed_link_with_hover() -> (
        WidgetTree,
        WidgetId,
        Signal<bool>,
        Signal<bool>,
        std::rc::Rc<Cell<u32>>,
    ) {
        let probe: std::rc::Rc<std::cell::RefCell<Option<(Signal<bool>, Signal<bool>)>>> =
            std::rc::Rc::new(std::cell::RefCell::new(None));
        let hits = std::rc::Rc::new(Cell::new(0_u32));
        let counter = hits.clone();
        let mut theme = teksilo_core::presets::intui::light();
        theme.style_slots.link = Some(std::rc::Rc::new(PressProbe(probe.clone())));
        let mut tree = WidgetTree::new().with_theme(theme);
        let link = tree.add(
            Link::new(lit!("Read more")).on_activate_fn(move |_| counter.set(counter.get() + 1)),
        );
        tree.layout(SizeProposal::exact(200.0, 60.0));
        let (pressed, hovered) = probe.borrow().clone().expect("style ran");
        (tree, link, pressed, hovered, hits)
    }

    fn probed_link() -> (WidgetTree, WidgetId, Signal<bool>, std::rc::Rc<Cell<u32>>) {
        let (tree, link, pressed, _hovered, hits) = probed_link_with_hover();
        (tree, link, pressed, hits)
    }

    /// Where the link comes to rest after it is followed — its own copy of the
    /// button family's `on_tap` resting-state rule. A link's hover state is its
    /// underline, so resting in the wrong one is not a subtle tint: a
    /// finger-tapped link that rests hovered stays underlined with nothing
    /// touching it.
    #[test]
    fn a_mouse_follow_rests_hovered_and_a_finger_follow_rests_idle() {
        use crate::button::press_test_support::touch_tap;

        let (mut tree, link, pressed, hovered, hits) = probed_link_with_hover();
        let at = tree.bounds(link).center();
        tree.pointer_move(at);
        assert!(hovered.get(), "the pointer arrived over the link");
        tree.pointer_down_button(at, teksilo_core::event::PointerButton::Primary);
        tree.pointer_up_button(at, teksilo_core::event::PointerButton::Primary);
        assert_eq!(hits.get(), 1, "the release followed the link");
        assert!(!pressed.get());
        assert!(
            hovered.get(),
            "a mouse that clicked the link is still on it, so it rests hovered",
        );

        let (mut tree, link, pressed, hovered, hits) = probed_link_with_hover();
        let at = tree.bounds(link).center();
        touch_tap(&mut tree, at);
        assert_eq!(hits.get(), 1, "the contact followed on its release");
        assert!(!pressed.get());
        assert!(
            !hovered.get(),
            "a finger leaves nothing behind, so the link must rest idle",
        );
    }

    /// A mouse press lights the link's `is_pressed` — the state its style has
    /// always been handed and which, before the controls sweep, only a keyboard
    /// `Space` or `Enter` could set. Following the link still lands on the
    /// release.
    #[test]
    fn a_mouse_press_lights_the_pressed_state_and_the_release_follows() {
        let (mut tree, link, pressed, hits) = probed_link();
        let at = tree.bounds(link).center();
        tree.pointer_move(at);
        tree.pointer_down_button(at, teksilo_core::event::PointerButton::Primary);
        assert!(pressed.get());
        assert_eq!(hits.get(), 0);
        tree.pointer_up_button(at, teksilo_core::event::PointerButton::Primary);
        assert!(!pressed.get());
        assert_eq!(hits.get(), 1);
    }

    /// A finger follows the link on the release, and a slide-off abandons it.
    #[test]
    fn a_touch_tap_follows_on_release_and_a_slide_off_abandons_it() {
        use crate::button::press_test_support::{finger, touch};
        use teksilo_core::pointer::PointerPhase;

        let (mut tree, link, pressed, hits) = probed_link();
        let bounds = tree.bounds(link);
        let at = bounds.center();
        let away = teksilo_canvas::Point::new(at.x, bounds.y + bounds.height + 80.0);

        let id = finger();
        tree.dispatch_pointer(touch(id, PointerPhase::Down, at, 0));
        assert!(pressed.get());
        tree.dispatch_pointer(touch(id, PointerPhase::Move, away, 20));
        assert!(!pressed.get());
        tree.dispatch_pointer(touch(id, PointerPhase::Up, away, 40));
        assert_eq!(hits.get(), 0);

        let id = finger();
        tree.dispatch_pointer(touch(id, PointerPhase::Down, at, 100));
        tree.dispatch_pointer(touch(id, PointerPhase::Up, at, 130));
        assert_eq!(hits.get(), 1);
    }
}
