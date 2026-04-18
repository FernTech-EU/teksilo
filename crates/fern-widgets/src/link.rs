//! Link — a clickable text label with underline.
//!
//! Follows the Button pattern for interaction but renders as underlined text.
//! V2 attached handlers — no event() override.

use fern_canvas::{Rect, Size, SizeProposal};
use fern_core::accessibility::AccessNodeBuilder;
use fern_core::build_context::BuildContext;
use fern_core::event::{EventResponse, Key, WidgetEvent};
use fern_core::signal::Signal;
use fern_core::widget::{CursorIcon, EventContext, LayoutContext, Widget, WidgetPlacement};
use fern_core::widget_builder::HandlerSet;
use fern_core::widget_id::WidgetId;
use fern_tokens::Color;

use crate::button::InteractionState;
use fern_tokens::CornerRadius;

use crate::primitives::{RectWidget, TextWidget, VStack, ZStack};

type CommandFactory = Box<dyn Fn(&mut EventContext)>;

/// A clickable text link with underline.
pub struct Link {
    text: String,
    url: Option<String>,
    action: Option<CommandFactory>,
    tooltip_text: Option<String>,
    rich_tooltip_source: Option<crate::tooltip::RichTooltipSource>,
    interaction: Option<Signal<InteractionState>>,
    root_child_id: Option<WidgetId>,
}

impl Link {
    pub fn new(text: impl Into<fern_i18n::LocalizedString>) -> Self {
        let ls: fern_i18n::LocalizedString = text.into();
        Self {
            text: ls.resolve_now(),
            url: None,
            action: None,
            tooltip_text: None,
            rich_tooltip_source: None,
            interaction: None,
            root_child_id: None,
        }
    }

    /// Shim (permanent, `#[doc(hidden)]`) — wraps a raw string in `LocalizedString::literal`.
    #[doc(hidden)]
    pub fn new_literal(text: impl Into<String>) -> Self {
        Self::new(fern_i18n::LocalizedString::literal(text))
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

    pub fn tooltip(mut self, text: impl Into<fern_i18n::LocalizedString>) -> Self {
        let ls: fern_i18n::LocalizedString = text.into();
        self.tooltip_text = Some(ls.resolve_now());
        self.rich_tooltip_source = None;
        self
    }

    /// Shim (permanent, `#[doc(hidden)]`) for `tooltip(...)` accepting a raw string.
    #[doc(hidden)]
    pub fn tooltip_literal(mut self, text: impl Into<String>) -> Self {
        self.tooltip_text = Some(text.into());
        self.rich_tooltip_source = None;
        self
    }

    /// Attach a rich tooltip resolved from the app-wide tooltip
    /// registry. See [`Button::rich_tooltip`](crate::button::Button::rich_tooltip).
    pub fn rich_tooltip(mut self, key: impl Into<String>) -> Self {
        self.rich_tooltip_source = Some(crate::tooltip::RichTooltipSource::Key(key.into()));
        self.tooltip_text = None;
        self
    }

    /// Attach a rich tooltip driven by inline `TooltipContent`.
    pub fn rich_tooltip_content(mut self, content: crate::tooltip::TooltipContent) -> Self {
        self.rich_tooltip_source = Some(crate::tooltip::RichTooltipSource::Content(content));
        self.tooltip_text = None;
        self
    }

    /// Get the URL, if set.
    pub fn get_url(&self) -> Option<&str> {
        self.url.as_deref()
    }
}

impl std::fmt::Debug for Link {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Link").field("text", &self.text).finish()
    }
}

fn resolve_link_color(state: InteractionState, colors: &fern_tokens::ColorTokens) -> Color {
    match state {
        InteractionState::Disabled => colors.text_disabled,
        InteractionState::Hovered | InteractionState::Pressed => colors.text_link_hover,
        InteractionState::Focused | InteractionState::Idle => colors.text_link,
    }
}

impl Widget for Link {
    fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
        let theme = ctx.theme().clone();
        let interaction = ctx.signal(InteractionState::Idle);
        self.interaction = Some(interaction.clone());

        let text_color = {
            let colors = theme.colors.clone();
            interaction.map(move |s| resolve_link_color(*s, &colors))
        };

        let underline_color = {
            let colors = theme.colors.clone();
            interaction.map(move |s| resolve_link_color(*s, &colors))
        };

        let text = TextWidget::new_literal(&self.text)
            .style(theme.typography.body.clone())
            .bind_color(text_color)
            .single_line()
            .a11y_hidden();
        let text_id = ctx.add(text);

        // 1px underline below the text
        let underline = RectWidget::new().bind_background(underline_color);
        let underline_id = ctx.add(underline);
        let underline_sized = ctx.add(
            crate::primitives::FixedSize::new()
                .bind_height(1.0_f32)
                .child_id(underline_id),
        );

        let content_id = ctx.add(
            VStack::new()
                .spacing(0.0)
                .add_child(text_id)
                .add_child(underline_sized),
        );

        // Int UI focus convention: the link paints its own
        // accent-colored border on focus instead of a separate
        // ring. Link has no visible rest-state border; the focus
        // state swaps in a `focus_ring_width` outline around the
        // text.
        let focus_ring_color = theme.colors.focus_ring;
        let focus_border_color = interaction.map(move |s| match s {
            InteractionState::Focused => focus_ring_color,
            _ => Color::TRANSPARENT,
        });
        let focus_bw = theme.shape.focus_ring_width;
        let focus_border_width = interaction.map(move |s| match s {
            InteractionState::Focused => focus_bw,
            _ => 0.0,
        });
        let focus_rect_id = ctx.add(
            RectWidget::new()
                .bind_border_color(focus_border_color)
                .bind_border_width(focus_border_width)
                .corner_radius(CornerRadius::uniform(
                    theme.components.link.corner_radius,
                )),
        );
        let root_id = ctx.add(
            ZStack::new()
                .add_child(focus_rect_id)
                .add_child(content_id),
        );

        if let Some(source) = self.rich_tooltip_source.take() {
            crate::tooltip::attach_rich_tooltip_source(
                ctx,
                root_id,
                source,
                crate::tooltip::DEFAULT_RICH_TOOLTIP_DELAY,
            );
        } else if let Some(ref tooltip_text) = self.tooltip_text {
            let tw = crate::tooltip::TooltipWidget::new_literal(tooltip_text);
            let tid = ctx.add(tw);
            ctx.attach_tooltip(root_id, tid, std::time::Duration::from_millis(500));
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
                move |action: fern_core::accesskit::Action,
                      ctx: &mut EventContext|
                      -> EventResponse {
                    if action == fern_core::accesskit::Action::Click {
                        if let Some(ref act) = *action_for_access {
                            act(ctx);
                        }
                        EventResponse::Handled
                    } else {
                        EventResponse::Ignored
                    }
                }
            })
            .focusable(true)
            .cursor(CursorIcon::Pointer);

        ctx.apply_self_handlers(handler_set);

        vec![root_id]
    }

    fn size_that_fits(&self, proposal: SizeProposal, ctx: &LayoutContext) -> Size {
        if let Some(root) = self.root_child_id
            && let Some(size) = ctx.child_size(root, proposal)
        {
            return size;
        }
        proposal.resolve(0.0, 0.0)
    }

    fn place_children(
        &self,
        bounds: Rect,
        _proposal: SizeProposal,
        children: &mut [WidgetPlacement],
        _ctx: &LayoutContext,
    ) {
        for child in children.iter_mut() {
            child.origin = fern_canvas::Point::new(bounds.x, bounds.y);
            child.size = Size::new(bounds.width, bounds.height);
        }
    }

    fn accessibility(&self, builder: &mut AccessNodeBuilder) {
        builder.set_role(fern_core::accesskit::Role::Link);
        builder.set_name(&self.text);
        builder.add_action(fern_core::accesskit::Action::Click);
        builder.add_action(fern_core::accesskit::Action::Focus);
    }

    fn children(&self) -> Vec<WidgetId> {
        self.root_child_id.into_iter().collect()
    }
}

