// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Link — a clickable text label with underline.
//!
//! Follows the Button pattern for interaction but renders as underlined text.
//! V2 attached handlers — no event() override.

use std::rc::Rc;

use bastyde_canvas::{Rect, Size, SizeProposal};
use bastyde_core::accessibility::AccessNodeBuilder;
use bastyde_core::build_context::BuildContext;
use bastyde_core::event::{EventResponse, Key, WidgetEvent};
use bastyde_core::signal::Signal;
use bastyde_core::styles::{LinkStyleConfig, SharedLinkStyle};
use bastyde_core::widget::{CursorIcon, EventContext, LayoutContext, Widget, WidgetPlacement};
use bastyde_core::widget_builder::HandlerSet;
use bastyde_core::widget_id::WidgetId;

use crate::button::InteractionState;
use bastyde_i18n::LocalizedString;

type CommandFactory = Box<dyn Fn(&mut EventContext)>;

/// A clickable text link with underline.
pub struct Link {
    text: LocalizedString,
    url: Option<String>,
    action: Option<CommandFactory>,
    tooltip_text: Option<LocalizedString>,
    rich_tooltip_source: Option<crate::tooltip::RichTooltipSource>,
    composite_tooltip_content: Option<Box<dyn bastyde_core::widget::Widget>>,
    interaction: Option<Signal<InteractionState>>,
    /// Visited state — orthogonal to `InteractionState`. The app owns
    /// the URL-visit tracking; this signal toggles `TextRole::LinkVisited`
    /// when no transient interaction (hover / press) is active.
    /// Default is a permanently-`false` signal so links that don't
    /// represent URLs render as unvisited.
    visited: Option<Signal<bool>>,
    /// Initial enabled-state; forwarded to the arena at build time.
    initial_enabled: bool,
    /// Per-call override for the link chrome.
    style_override: Option<SharedLinkStyle>,
    root_child_id: Option<WidgetId>,
}

impl Link {
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
            initial_enabled: true,
            style_override: None,
            root_child_id: None,
        }
    }

    /// Mark the link's target as visited. Drives `TextRole::LinkVisited`
    /// when no transient interaction (hover / press) is active. Visited
    /// is overridden by hover/press, following the web convention. The
    /// app owns the signal (typically backed by URL-history state).
    pub fn visited(mut self, visited: Signal<bool>) -> Self {
        self.visited = Some(visited);
        self
    }

    /// Per-call style override for the link chrome.
    pub fn style(mut self, style: impl bastyde_core::styles::LinkStyle) -> Self {
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
        content: impl bastyde_core::widget::Widget + 'static,
    ) -> Self {
        self.composite_tooltip_content = Some(Box::new(content));
        self.tooltip_text = None;
        self.rich_tooltip_source = None;
        self
    }

    /// Get the URL, if set.
    pub fn get_url(&self) -> Option<&str> {
        self.url.as_deref()
    }

    /// Set the initial enabled state. Forwarded to the arena at build
    /// time. For reactive enable/disable use `ctx.enabled_when(id, signal)`.
    pub fn enabled(mut self, enabled: bool) -> Self {
        self.initial_enabled = enabled;
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
        // Forward initial-enabled into the arena; see IconButton.
        if !self.initial_enabled {
            ctx.enabled_when(self_id, false);
        }
        let effective_enabled = ctx.effective_enabled_signal(self_id);

        let interaction = ctx.signal(InteractionState::Idle);
        self.interaction = Some(interaction.clone());

        // Derive the four state bools `LinkStyle` expects from the
        // single `InteractionState` signal. `is_disabled` derives
        // from the arena (reactive) instead of a build-time snapshot.
        let is_hovered = interaction.map(|s| matches!(s, InteractionState::Hovered));
        let is_pressed = interaction.map(|s| matches!(s, InteractionState::Pressed));
        let is_focused = interaction.map(|s| matches!(s, InteractionState::Focused));
        let is_visited = self.visited.clone().unwrap_or_else(|| Signal::new(false));
        let is_disabled = effective_enabled.map(|on| !*on);

        let style: SharedLinkStyle = self
            .style_override
            .clone()
            .or_else(|| ctx.theme().style_slots.link.clone())
            .unwrap_or_else(|| Rc::new(crate::styles::RecipeLinkStyle));
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
            let tw = crate::tooltip::TooltipWidget::new(tooltip_text);
            let tid = ctx.add(tw);
            let delay = ctx.theme().motion.tooltip_delay;
            ctx.attach_tooltip(root_id, tid, delay);
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

        let handler_set = HandlerSet::new()
            .on_tap({
                move |_pos, ctx: &mut EventContext| {
                    if let Some(ref action) = *action_for_tap {
                        action(ctx);
                    }
                    int_tap.set(InteractionState::Hovered);
                }
            })
            .on_hover({
                move |entered: bool, _ctx: &mut EventContext| {
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
                move |action: bastyde_core::accesskit::Action,
                      ctx: &mut EventContext|
                      -> EventResponse {
                    if action == bastyde_core::accesskit::Action::Click {
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
    ) -> bastyde_core::widget::LayoutResponse {
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
            child.origin = bastyde_canvas::Point::new(bounds.x, bounds.y);
            child.size = Size::new(bounds.width, bounds.height);
        }
    }

    fn accessibility(&self, builder: &mut AccessNodeBuilder) {
        builder.set_role(bastyde_core::accesskit::Role::Link);
        builder.set_name(self.text.resolve_now());
        if let Some(ref url) = self.url {
            builder.set_url(url.clone());
        }
        // Framework a11y walker sets `set_disabled` from arena state.
        // Actions are always advertised — when disabled the framework
        // gates them at dispatch via `arena.is_enabled`.
        builder.add_action(bastyde_core::accesskit::Action::Click);
        builder.add_action(bastyde_core::accesskit::Action::Focus);
    }

    fn children(&self) -> Vec<WidgetId> {
        self.root_child_id.into_iter().collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bastyde_core::event::Modifiers;
    use bastyde_core::widget_tree::WidgetTree;
    use bastyde_i18n::lit;
    use std::cell::Cell;

    #[test]
    fn keyup_without_keydown_does_not_fire() {
        // Lone-KeyUp guard: when a shortcut consumes the KeyDown and
        // focus returns to the link, the trailing KeyUp must NOT activate.
        let mut tree = WidgetTree::new().with_theme(bastyde_core::presets::intui::light());
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
}
