//! CommandLinkButton — large two-line button with icon, title, and
//! subtitle. Used for wizard landing screens, onboarding choices, and
//! any "card-shaped CTA" pattern.
//!
//! Modeled on Qt's `QCommandLinkButton`. Distinct from a regular
//! [`Button`](crate::button::Button) by its layout (`HStack(icon +
//! VStack(title + subtitle))`) and default visual variant (`Flat` —
//! Int UI convention — with an interactive surface tint on hover).
//!
//! ```ignore
//! CommandLinkButton::new(tr!("create_new_project"))
//!     .description(tr!("create_new_project_subtitle"))
//!     .icon(IconWidget::from_svg(NEW_PROJECT_ICON))
//!     .on_activate_fn(|ctx| ctx.send_intent(AppIntent::NewProject))
//! ```

use fern_canvas::{Rect, SizeProposal};
use fern_core::accessibility::AccessNodeBuilder;
use fern_core::build_context::BuildContext;
use fern_core::event::{EventResponse, Key, WidgetEvent};
use fern_core::signal::Signal;
use fern_core::widget::{CursorIcon, EventContext, LayoutContext, Widget, WidgetPlacement};
use fern_core::widget_builder::HandlerSet;
use fern_core::widget_id::WidgetId;
use fern_tokens::{
    BorderRole, CornerRadius, HAlignment, SurfaceRole, TextRole, TextStyleRole, VAlignment,
};

use crate::button::InteractionState;
use crate::primitives::icon_widget::IconWidget;
use crate::primitives::{HStack, Padding, RectWidget, TextWidget, VStack, ZStack};

/// A large two-line CTA button: icon + title + subtitle.
pub struct CommandLinkButton {
    title: String,
    description: Option<String>,
    icon: Option<IconWidget>,
    enabled: bool,
    action: Option<Box<dyn Fn(&mut EventContext)>>,
    interaction: Signal<InteractionState>,
    root_child_id: Option<WidgetId>,
}

impl CommandLinkButton {
    pub fn new(title: impl Into<fern_i18n::LocalizedString>) -> Self {
        let ls: fern_i18n::LocalizedString = title.into();
        Self {
            title: ls.resolve_now(),
            description: None,
            icon: None,
            enabled: true,
            action: None,
            interaction: Signal::new(InteractionState::Idle),
            root_child_id: None,
        }
    }

    /// Shim (permanent, `#[doc(hidden)]`) — wraps a raw title in
    /// `LocalizedString::literal`.
    #[doc(hidden)]
    pub fn new_literal(title: impl Into<String>) -> Self {
        Self::new(fern_i18n::LocalizedString::literal(title))
    }

    /// Optional descriptive subtitle rendered below the title.
    pub fn description(mut self, text: impl Into<fern_i18n::LocalizedString>) -> Self {
        let ls: fern_i18n::LocalizedString = text.into();
        self.description = Some(ls.resolve_now());
        self
    }

    /// Shim (permanent, `#[doc(hidden)]`) for `description(...)`.
    #[doc(hidden)]
    pub fn description_literal(self, text: impl Into<String>) -> Self {
        self.description(fern_i18n::LocalizedString::literal(text))
    }

    /// Leading icon — large enough to anchor the card visually
    /// (rendered at 28 dp).
    pub fn icon(mut self, icon: IconWidget) -> Self {
        self.icon = Some(icon);
        self
    }

    pub fn enabled(mut self, enabled: bool) -> Self {
        self.enabled = enabled;
        self
    }

    /// Closure invoked on activation. Use `ctx.send_intent(...)` to
    /// route through the Action / Intent system.
    pub fn on_activate_fn(mut self, f: impl Fn(&mut EventContext) + 'static) -> Self {
        self.action = Some(Box::new(f));
        self
    }
}

impl std::fmt::Debug for CommandLinkButton {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CommandLinkButton")
            .field("title", &self.title)
            .field("description", &self.description)
            .field("enabled", &self.enabled)
            .finish()
    }
}

fn resolve_bg_role(state: InteractionState) -> SurfaceRole {
    match state {
        InteractionState::Pressed => SurfaceRole::Pressed,
        InteractionState::Hovered => SurfaceRole::Hover,
        _ => SurfaceRole::Transparent,
    }
}

fn resolve_border_role(state: InteractionState) -> BorderRole {
    match state {
        InteractionState::Focused => BorderRole::Focused,
        _ => BorderRole::Transparent,
    }
}

impl Widget for CommandLinkButton {
    fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
        let enabled = self.enabled;
        let interaction = ctx.signal(if enabled {
            InteractionState::Idle
        } else {
            InteractionState::Disabled
        });
        self.interaction = interaction.clone();

        let bg_role = interaction.map(|s| resolve_bg_role(*s));
        let border_role = interaction.map(|s| resolve_border_role(*s));
        let title_role = interaction.map(|s| match s {
            InteractionState::Disabled => TextRole::Disabled,
            _ => TextRole::Primary,
        });
        let desc_role = interaction.map(|s| match s {
            InteractionState::Disabled => TextRole::Disabled,
            _ => TextRole::Secondary,
        });
        let icon_role = title_role.clone();

        let style = ctx.theme().components.command_link_button;
        let normal_bw = ctx.theme().components.button.border_width;
        let focus_bw = ctx.theme().shape.focus_ring_width;
        let border_width = interaction.map(move |s| match s {
            InteractionState::Focused => focus_bw,
            _ => normal_bw,
        });
        let corner_radius = ctx.theme().components.button.corner_radius;

        // Title + optional description column.
        let title_widget = TextWidget::new_literal(&self.title)
            .style(TextStyleRole::BodyBold)
            .bind_color(title_role)
            .single_line()
            .a11y_hidden();
        let title_id = ctx.add(title_widget);

        let mut text_column = VStack::new()
            .spacing(style.title_description_gap)
            .alignment(HAlignment::Leading)
            .add_child(title_id);
        if let Some(description) = &self.description {
            let desc = ctx.add(
                TextWidget::new_literal(description)
                    .style(TextStyleRole::Body)
                    .bind_color(desc_role)
                    .a11y_hidden(),
            );
            text_column = text_column.add_child(desc);
        }
        let text_column_id = ctx.add(text_column);

        // Optional leading icon.
        let mut row = HStack::new()
            .spacing(style.icon_text_gap)
            .alignment(VAlignment::Center);
        if let Some(icon) = self.icon.take() {
            let icon_id = ctx.add(icon.icon_size(style.icon_size).bind_color(icon_role));
            row = row.add_child(icon_id);
        }
        row = row.add_child(text_column_id);
        let row_id = ctx.add(row);

        // Padding inside the surface.
        let padded = ctx.add(
            Padding::symmetric(style.padding_vertical, style.padding_horizontal).child_id(row_id),
        );

        // Surface (background + border, drives hover / press / focus).
        let rect = ctx.add(
            RectWidget::new()
                .bind_background(bg_role)
                .bind_border_color(border_role)
                .bind_border_width(border_width)
                .corner_radius(CornerRadius::uniform(corner_radius)),
        );

        let zstack = ctx.add(ZStack::new().add_child(rect).add_child(padded));
        let root = ctx.add(crate::primitives::MinSize::new(0.0, style.min_height).child_id(zstack));

        // Attached handlers — same shape as Button but without
        // shortcut / tooltip / has_popup machinery.
        let action = self.action.take();
        let action_rc: std::rc::Rc<Option<Box<dyn Fn(&mut EventContext)>>> =
            std::rc::Rc::new(action);
        let action_for_tap = action_rc.clone();
        let action_for_key = action_rc.clone();
        let action_for_access = action_rc.clone();

        let int_tap = interaction.clone();
        let int_hover_enter = interaction.clone();
        let int_hover_leave = interaction.clone();
        let int_key = interaction.clone();
        let int_focus = interaction.clone();

        let handlers = HandlerSet::new()
            .focusable(enabled)
            .cursor(if enabled {
                CursorIcon::Pointer
            } else {
                CursorIcon::Default
            })
            .on_tap(move |_ev: &fern_core::TapEvent, ctx: &mut EventContext| {
                if !enabled {
                    return;
                }
                if let Some(ref a) = *action_for_tap {
                    a(ctx);
                }
                int_tap.set(InteractionState::Hovered);
            })
            .on_hover(move |entered: bool, _ctx: &mut EventContext| {
                if !enabled {
                    return;
                }
                if entered {
                    int_hover_enter.set(InteractionState::Hovered);
                } else {
                    int_hover_leave.set(InteractionState::Idle);
                }
            })
            .on_key(
                move |event: &WidgetEvent, ctx: &mut EventContext| -> EventResponse {
                    if !enabled {
                        return EventResponse::Ignored;
                    }
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
                            if int_key.get() != InteractionState::Pressed {
                                return EventResponse::Ignored;
                            }
                            if let Some(ref a) = *action_for_key {
                                a(ctx);
                            }
                            int_key.set(InteractionState::Focused);
                            EventResponse::Handled
                        }
                        _ => EventResponse::Ignored,
                    }
                },
            )
            .on_focus(move |gained: bool, _ctx: &mut EventContext| {
                if gained {
                    if int_focus.get() == InteractionState::Idle {
                        int_focus.set(InteractionState::Focused);
                    }
                } else {
                    int_focus.set(InteractionState::Idle);
                }
            })
            .on_access_action(
                move |action: fern_core::accesskit::Action,
                      ctx: &mut EventContext|
                      -> EventResponse {
                    if action == fern_core::accesskit::Action::Click && enabled {
                        if let Some(ref a) = *action_for_access {
                            a(ctx);
                        }
                        EventResponse::Handled
                    } else {
                        EventResponse::Ignored
                    }
                },
            );
        ctx.apply_self_handlers(handlers);

        self.root_child_id = Some(root);
        vec![root]
    }

    fn layout_response(
        &self,
        proposal: SizeProposal,
        ctx: &LayoutContext,
    ) -> fern_core::widget::LayoutResponse {
        let min_height = ctx.theme.components.command_link_button.min_height;
        self.root_child_id
            .and_then(|id| ctx.child_size(id, proposal))
            .unwrap_or_else(|| proposal.resolve(0.0, min_height))
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
        builder.set_role(fern_core::accesskit::Role::Button);
        // Compose the AT name as "title — description" so screen reader
        // users hear both lines without having to drill into children.
        let name = match &self.description {
            Some(desc) => format!("{} — {}", self.title, desc),
            None => self.title.clone(),
        };
        builder.set_name(name);
    }

    fn children(&self) -> Vec<WidgetId> {
        self.root_child_id.into_iter().collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use fern_core::widget_tree::WidgetTree;
    use fern_tokens::Theme;

    #[test]
    fn builds_with_title_and_description() {
        let mut tree = WidgetTree::new().with_theme(Theme::light_default());
        let id = tree.add(
            CommandLinkButton::new_literal("Create new project")
                .description_literal("Start with a blank workspace."),
        );
        tree.layout(SizeProposal {
            width: Some(420.0),
            height: None,
        });
        let b = tree.bounds(id);
        assert!(b.width > 0.0);
        let min_height = Theme::light_default()
            .components
            .command_link_button
            .min_height;
        assert!(b.height >= min_height);
    }

    #[test]
    fn a11y_role_is_button_with_combined_name() {
        let mut tree = WidgetTree::new().with_theme(Theme::light_default());
        let id = tree.add(
            CommandLinkButton::new_literal("Create new project")
                .description_literal("Start blank."),
        );
        tree.layout(SizeProposal::exact(400.0, 100.0));
        let info = tree.accessibility_node(id);
        assert_eq!(info.role(), fern_core::accesskit::Role::Button);
        assert_eq!(info.name(), Some("Create new project — Start blank."));
    }

    #[test]
    fn click_via_access_action_invokes_callback() {
        use std::cell::Cell;
        use std::rc::Rc;
        let fired = Rc::new(Cell::new(0usize));
        let fired_clone = fired.clone();
        let mut tree = WidgetTree::new().with_theme(Theme::light_default());
        let id = tree.add(
            CommandLinkButton::new_literal("Open existing project")
                .on_activate_fn(move |_| fired_clone.set(fired_clone.get() + 1)),
        );
        tree.layout(SizeProposal::exact(400.0, 100.0));
        tree.dispatch_event(WidgetEvent::AccessAction {
            action: fern_core::accesskit::Action::Click,
            target: Some(id),
            target_node: fern_core::accessibility::root_node_id(),
            data: None,
        });
        assert_eq!(fired.get(), 1);
    }
}
