// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

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
//! CommandLinkButton::new(tr!(create_new_project()))
//!     .description(tr!(create_new_project_subtitle()))
//!     .icon(IconWidget::from_svg(NEW_PROJECT_ICON))
//!     .on_activate_fn(|ctx| ctx.send_intent(AppIntent::NewProject))
//! ```

use bastyde_canvas::{Rect, SizeProposal};
use bastyde_core::accessibility::AccessNodeBuilder;
use bastyde_core::build_context::BuildContext;
use bastyde_core::signal::Signal;
use bastyde_core::widget::{EventContext, LayoutContext, Widget, WidgetPlacement};
use bastyde_core::widget_id::WidgetId;
use bastyde_tokens::{
    BorderRole, CornerRadius, HAlignment, SurfaceRole, TextRole, TextStyleRole, VAlignment,
};

use crate::button::InteractionState;
use crate::primitives::icon_widget::IconWidget;
use crate::primitives::{HStack, Padding, RectWidget, TextWidget, VStack, ZStack};
use bastyde_i18n::LocalizedString;

/// CommandLinkButton design tokens. The widget is a group-4 composite
/// with no dedicated recipe module.
pub const COMMAND_LINK_BUTTON_ICON_SIZE: f32 = 28.0;
pub const COMMAND_LINK_BUTTON_ICON_TEXT_GAP: f32 = 14.0;
pub const COMMAND_LINK_BUTTON_TITLE_DESCRIPTION_GAP: f32 = 4.0;
pub const COMMAND_LINK_BUTTON_PADDING_HORIZONTAL: f32 = 16.0;
pub const COMMAND_LINK_BUTTON_PADDING_VERTICAL: f32 = 14.0;
pub const COMMAND_LINK_BUTTON_MIN_HEIGHT: f32 = 64.0;

/// A large two-line CTA button: icon + title + subtitle.
pub struct CommandLinkButton {
    title: LocalizedString,
    description: Option<LocalizedString>,
    icon: Option<IconWidget>,
    /// Initial enabled-state; forwarded to the arena at build time.
    initial_enabled: bool,
    action: Option<Box<dyn Fn(&mut EventContext)>>,
    /// Per-call title text-style override. `None` ⇒ `TextStyleRole::BodyBold`.
    title_style: Option<bastyde_core::color_prop::TextStyleProp>,
    /// Per-call description text-style override. `None` ⇒ `TextStyleRole::Body`.
    description_style: Option<bastyde_core::color_prop::TextStyleProp>,
    /// Per-call title text-color override. `None` ⇒ `TextRole::Primary`.
    title_color: Option<bastyde_core::color_prop::ColorProp>,
    /// Per-call description text-color override. `None` ⇒ `TextRole::Secondary`.
    description_color: Option<bastyde_core::color_prop::ColorProp>,
    interaction: Signal<InteractionState>,
    root_child_id: Option<WidgetId>,
}

impl CommandLinkButton {
    pub fn new(title: impl Into<LocalizedString>) -> Self {
        let ls: LocalizedString = title.into();
        Self {
            title: ls,
            description: None,
            icon: None,
            initial_enabled: true,
            action: None,
            title_style: None,
            description_style: None,
            title_color: None,
            description_color: None,
            interaction: Signal::new(InteractionState::Idle),
            root_child_id: None,
        }
    }

    /// Optional descriptive subtitle rendered below the title.
    pub fn description(mut self, text: impl Into<LocalizedString>) -> Self {
        let ls: LocalizedString = text.into();
        self.description = Some(ls);
        self
    }

    /// Leading icon — large enough to anchor the card visually
    /// (rendered at 28 dp).
    pub fn icon(mut self, icon: IconWidget) -> Self {
        self.icon = Some(icon);
        self
    }

    /// Set the initial enabled state. Forwarded to the arena at build
    /// time. Use `ctx.enabled_when(button_id, signal)` for reactivity.
    pub fn enabled(mut self, enabled: bool) -> Self {
        self.initial_enabled = enabled;
        self
    }

    /// Closure invoked on activation. Use `ctx.send_intent(...)` to
    /// route through the Action / Intent system.
    pub fn on_activate_fn(mut self, f: impl Fn(&mut EventContext) + 'static) -> Self {
        self.action = Some(Box::new(f));
        self
    }

    /// Override the title's text style (font, size, weight). Accepts a
    /// `TextStyleRole`, a `TextStyle`, or a `Signal` of either. Default
    /// (unset) is `TextStyleRole::BodyBold`.
    pub fn title_style(mut self, style: impl Into<bastyde_core::color_prop::TextStyleProp>) -> Self {
        self.title_style = Some(style.into());
        self
    }

    /// Override the description's text style. Default is `TextStyleRole::Body`.
    pub fn description_style(
        mut self,
        style: impl Into<bastyde_core::color_prop::TextStyleProp>,
    ) -> Self {
        self.description_style = Some(style.into());
        self
    }

    /// Override the title's text color. Accepts `Color`, a role, or a
    /// `Signal` of either. Default (unset) is `TextRole::Primary`.
    pub fn title_color(mut self, color: impl Into<bastyde_core::color_prop::ColorProp>) -> Self {
        self.title_color = Some(color.into());
        self
    }

    /// Override the description's text color. Default is `TextRole::Secondary`.
    pub fn description_color(
        mut self,
        color: impl Into<bastyde_core::color_prop::ColorProp>,
    ) -> Self {
        self.description_color = Some(color.into());
        self
    }
}

impl std::fmt::Debug for CommandLinkButton {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CommandLinkButton")
            .field("title", &self.title)
            .field("description", &self.description)
            .field("initial_enabled", &self.initial_enabled)
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
        let self_id = ctx.self_id();
        // Forward initial-enabled to the arena; see IconButton.
        if !self.initial_enabled {
            ctx.enabled_when(self_id, false);
        }

        let interaction = ctx.signal(InteractionState::Idle);
        self.interaction = interaction.clone();

        // The leaves' `ColorProp::resolve(theme, ctx.effective_enabled)`
        // substitutes `TextRole::Disabled` automatically when the arena
        // says we're disabled; we no longer need to fold the Disabled
        // state into these role-derivations.
        let bg_role = interaction.map(|s| resolve_bg_role(*s));
        let border_role = interaction.map(|s| resolve_border_role(*s));
        let title_role = interaction.map(|_s| TextRole::Primary);
        let desc_role = interaction.map(|_s| TextRole::Secondary);
        let icon_role = title_role.clone();

        let normal_bw = crate::styles::recipe_button_style::BUTTON_BORDER_WIDTH;
        let focus_bw = ctx.theme().shape.focus_ring_width;
        let border_width = interaction.map(move |s| match s {
            InteractionState::Focused => focus_bw,
            _ => normal_bw,
        });
        let corner_radius = crate::styles::recipe_button_style::BUTTON_CORNER_RADIUS;

        // Title + optional description column.
        let title_color: bastyde_core::color_prop::ColorProp =
            self.title_color.clone().unwrap_or_else(|| title_role.into());
        let title_style: bastyde_core::color_prop::TextStyleProp = self
            .title_style
            .clone()
            .unwrap_or_else(|| TextStyleRole::BodyBold.into());
        let title_widget = TextWidget::new(self.title.clone())
            .style(title_style)
            .bind_color(title_color)
            .single_line()
            .a11y_hidden();
        let title_id = ctx.add(title_widget);

        let mut text_column = VStack::new()
            .spacing(COMMAND_LINK_BUTTON_TITLE_DESCRIPTION_GAP)
            .alignment(HAlignment::Leading)
            .add_child(title_id);
        if let Some(description) = &self.description {
            let desc_color: bastyde_core::color_prop::ColorProp = self
                .description_color
                .clone()
                .unwrap_or_else(|| desc_role.into());
            let desc_style: bastyde_core::color_prop::TextStyleProp = self
                .description_style
                .clone()
                .unwrap_or_else(|| TextStyleRole::Body.into());
            let desc = ctx.add(
                TextWidget::new(description.clone())
                    .style(desc_style)
                    .bind_color(desc_color)
                    .a11y_hidden(),
            );
            text_column = text_column.add_child(desc);
        }
        let text_column_id = ctx.add(text_column);

        // Optional leading icon.
        let mut row = HStack::new()
            .spacing(COMMAND_LINK_BUTTON_ICON_TEXT_GAP)
            .alignment(VAlignment::Center);
        if let Some(icon) = self.icon.take() {
            let icon_id = ctx.add(
                icon.icon_size(COMMAND_LINK_BUTTON_ICON_SIZE)
                    .bind_color(icon_role),
            );
            row = row.add_child(icon_id);
        }
        row = row.add_child(text_column_id);
        let row_id = ctx.add(row);

        // Padding inside the surface.
        let padded = ctx.add(
            Padding::symmetric(
                COMMAND_LINK_BUTTON_PADDING_VERTICAL,
                COMMAND_LINK_BUTTON_PADDING_HORIZONTAL,
            )
            .child_id(row_id),
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
        let root = ctx.add(
            crate::primitives::MinSize::new(0.0, COMMAND_LINK_BUTTON_MIN_HEIGHT).child_id(zstack),
        );

        // Attached handlers via the shared button-family helper
        // (`build_interaction_handlers`) — same interaction/keyboard/AT
        // contract as Button, including the lone-KeyUp guard. No
        // shortcut / tooltip / has_popup machinery here.
        let action: std::rc::Rc<Option<Box<dyn Fn(&mut EventContext)>>> =
            std::rc::Rc::new(self.action.take());
        let on_activate: std::rc::Rc<dyn Fn(&mut EventContext)> =
            std::rc::Rc::new(move |ctx: &mut EventContext| {
                if let Some(ref a) = *action {
                    a(ctx);
                }
            });
        let handlers = crate::button::build_interaction_handlers(interaction, on_activate, true);
        ctx.apply_self_handlers(handlers);

        self.root_child_id = Some(root);
        vec![root]
    }

    fn layout_response(
        &self,
        proposal: SizeProposal,
        ctx: &LayoutContext,
    ) -> bastyde_core::widget::LayoutResponse {
        let _ = ctx;
        self.root_child_id
            .and_then(|id| ctx.child_size(id, proposal))
            .unwrap_or_else(|| proposal.resolve(0.0, COMMAND_LINK_BUTTON_MIN_HEIGHT))
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
        builder.set_role(bastyde_core::accesskit::Role::Button);
        // Compose the AT name as "title — description" so screen reader
        // users hear both lines without having to drill into children.
        let name = match &self.description {
            Some(desc) => format!("{} — {}", self.title.resolve_now(), desc.resolve_now()),
            None => self.title.resolve_now(),
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
    use bastyde_core::event::WidgetEvent;
    use bastyde_core::widget_tree::WidgetTree;
    use bastyde_i18n::lit;

    #[test]
    fn builds_with_title_and_description() {
        let mut tree = WidgetTree::new().with_theme(bastyde_core::presets::intui::light());
        let id = tree.add(
            CommandLinkButton::new(lit!("Create new project"))
                .description(lit!("Start with a blank workspace.")),
        );
        tree.layout(SizeProposal {
            width: Some(420.0),
            height: None,
        });
        let b = tree.bounds(id);
        assert!(b.width > 0.0);
        let _ = bastyde_core::presets::intui::light();
        assert!(b.height >= COMMAND_LINK_BUTTON_MIN_HEIGHT);
    }

    #[test]
    fn a11y_role_is_button_with_combined_name() {
        let mut tree = WidgetTree::new().with_theme(bastyde_core::presets::intui::light());
        let id = tree.add(
            CommandLinkButton::new(lit!("Create new project")).description(lit!("Start blank.")),
        );
        tree.layout(SizeProposal::exact(400.0, 100.0));
        let info = tree.accessibility_node(id);
        assert_eq!(info.role(), bastyde_core::accesskit::Role::Button);
        assert_eq!(info.name(), Some("Create new project — Start blank."));
    }

    #[test]
    fn click_via_access_action_invokes_callback() {
        use std::cell::Cell;
        use std::rc::Rc;
        let fired = Rc::new(Cell::new(0usize));
        let fired_clone = fired.clone();
        let mut tree = WidgetTree::new().with_theme(bastyde_core::presets::intui::light());
        let id = tree.add(
            CommandLinkButton::new(lit!("Open existing project"))
                .on_activate_fn(move |_| fired_clone.set(fired_clone.get() + 1)),
        );
        tree.layout(SizeProposal::exact(400.0, 100.0));
        tree.dispatch_event(WidgetEvent::AccessAction {
            action: bastyde_core::accesskit::Action::Click,
            target: Some(id),
            target_node: bastyde_core::accessibility::root_node_id(),
            data: None,
        });
        assert_eq!(fired.get(), 1);
    }
}
