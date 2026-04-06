//! MenuItem widget — a single item in a menu or context menu.
//!
//! Non-generic, closure-based command erasure (same pattern as Button).
//! Supports icons, shortcut labels, disabled state, and submenu triggers.

use fern_canvas::{Rect, Size, SizeProposal};
use fern_core::accessibility::AccessNodeBuilder;
use fern_core::app_command::AppCommand;
use fern_core::build_context::BuildContext;
use fern_core::event::{EventResponse, Key, WidgetEvent};
use fern_core::signal::Signal;
use fern_core::overlay::{DismissBehavior, OverlayLayer, OverlayPlacement, OverlayRequest};
use fern_core::widget::{CursorIcon, EventContext, LayoutContext, Widget, WidgetPlacement};
use fern_core::widget_builder::HandlerSet;
use fern_core::widget_id::WidgetId;
use fern_tokens::Color;

use crate::primitives::{HStack, IconWidget, Padding, RectWidget, Spacer, TextWidget, ZStack};

/// Type-erased command factory (same as Button).
type CommandFactory = Box<dyn Fn(&mut EventContext)>;

/// Interaction state for a menu item.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum MenuItemState {
    Idle,
    Hovered,
    Pressed,
    Disabled,
}

/// A single menu item: icon + label + shortcut label + optional submenu chevron.
pub struct MenuItem {
    label: String,
    icon: Option<IconWidget>,
    shortcut_label: Option<String>,
    action: Option<CommandFactory>,
    enabled: bool,
    submenu_factory: Option<Box<dyn Fn() -> Box<dyn Widget>>>,
    // Build state
    interaction: Signal<MenuItemState>,
    root_child_id: Option<WidgetId>,
    submenu_content_id: Option<WidgetId>,
}

impl MenuItem {
    pub fn new(label: impl Into<String>) -> Self {
        Self {
            label: label.into(),
            icon: None,
            shortcut_label: None,
            action: None,
            enabled: true,
            submenu_factory: None,
            interaction: Signal::new(MenuItemState::Idle),
            root_child_id: None,
            submenu_content_id: None,
        }
    }

    /// Set the command to emit on activation. Generic only at this call site.
    pub fn on_activate<C: AppCommand>(mut self, command: C) -> Self {
        self.action = Some(Box::new(move |ctx: &mut EventContext| {
            ctx.emit(command.clone());
        }));
        self
    }

    /// Set a leading icon.
    pub fn icon(mut self, icon: IconWidget) -> Self {
        self.icon = Some(icon);
        self
    }

    /// Set a trailing shortcut label (e.g., "Ctrl+X").
    pub fn shortcut_label(mut self, label: impl Into<String>) -> Self {
        self.shortcut_label = Some(label.into());
        self
    }

    pub fn enabled(mut self, enabled: bool) -> Self {
        self.enabled = enabled;
        self
    }

    /// Create a submenu trigger item. The factory is invoked on hover to produce
    /// the submenu content (typically a `MenuList`).
    pub fn submenu(
        label: impl Into<String>,
        factory: impl Fn() -> Box<dyn Widget> + 'static,
    ) -> Self {
        Self {
            label: label.into(),
            icon: None,
            shortcut_label: None,
            action: None,
            enabled: true,
            submenu_factory: Some(Box::new(factory)),
            interaction: Signal::new(MenuItemState::Idle),
            root_child_id: None,
            submenu_content_id: None,
        }
    }

    /// Whether this is a submenu trigger.
    pub fn is_submenu(&self) -> bool {
        self.submenu_factory.is_some()
    }
}

impl std::fmt::Debug for MenuItem {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MenuItem")
            .field("label", &self.label)
            .field("enabled", &self.enabled)
            .field("is_submenu", &self.submenu_factory.is_some())
            .finish()
    }
}

fn resolve_bg(state: MenuItemState, on_surface: Color) -> Color {
    match state {
        MenuItemState::Idle => Color::TRANSPARENT,
        MenuItemState::Hovered => on_surface.with_alpha(0.08),
        MenuItemState::Pressed => on_surface.with_alpha(0.12),
        MenuItemState::Disabled => Color::TRANSPARENT,
    }
}

fn resolve_text(state: MenuItemState, text_color: Color, disabled_color: Color) -> Color {
    match state {
        MenuItemState::Disabled => disabled_color,
        _ => text_color,
    }
}

impl Widget for MenuItem {
    fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
        let theme = ctx.theme().clone();
        let enabled = self.enabled;

        let interaction = ctx.signal(if enabled {
            MenuItemState::Idle
        } else {
            MenuItemState::Disabled
        });
        self.interaction = interaction.clone();

        let on_surface = theme.colors.on_surface;
        let bg_color = {
            let on_surface = on_surface;
            interaction.map(move |s| resolve_bg(*s, on_surface))
        };

        let text_color = {
            let text = theme.colors.on_surface;
            let disabled = theme.colors.disabled_text;
            interaction.map(move |s| resolve_text(*s, text, disabled))
        };

        // Build the row: [icon | label | Spacer | shortcut | chevron]
        let mut row = HStack::new().spacing(8.0);

        // Leading icon (fixed 16px)
        if let Some(icon) = self.icon.take() {
            let icon = icon.bind_color(text_color.clone());
            row = row.child(icon);
        }

        // Label
        let label = TextWidget::new(&self.label)
            .style(theme.typography.body.clone())
            .bind_color(text_color.clone());
        row = row.child(label);

        // Spacer between label and trailing content
        row = row.child(Spacer::new());

        // Shortcut label (dimmed)
        if let Some(ref shortcut_text) = self.shortcut_label {
            let shortcut_color = {
                let text = theme.colors.on_surface.with_alpha(0.5);
                let disabled = theme.colors.disabled_text;
                interaction.map(move |s| resolve_text(*s, text, disabled))
            };
            let shortcut = TextWidget::new(shortcut_text)
                .style(theme.typography.label.clone())
                .bind_color(shortcut_color);
            row = row.child(shortcut);
        }

        // Pre-create submenu content if this is a submenu trigger
        let submenu_content_id = if let Some(factory) = self.submenu_factory.take() {
            let submenu_widget = factory();
            let id = ctx.add_boxed(submenu_widget);
            ctx.set_dormant(id);
            self.submenu_content_id = Some(id);
            let chevron = IconWidget::chevron_right(12.0).bind_color(text_color);
            row = row.child(chevron);
            Some(id)
        } else {
            None
        };

        let row_id = ctx.add(row);

        let padding = Padding::symmetric(6.0, 12.0).set_child(row_id);
        let padding_id = ctx.add(padding);

        // Background rect
        let rect = RectWidget::new().bind_background(bg_color);
        let rect_id = ctx.add(rect);

        let zstack = ZStack::new().add_child(rect_id).add_child(padding_id);
        let root_id = ctx.add(zstack);

        self.root_child_id = Some(root_id);

        // --- Handlers ---
        let action = self.action.take();
        let action_rc: std::rc::Rc<Option<CommandFactory>> = std::rc::Rc::new(action);
        let action_for_tap = action_rc.clone();
        let action_for_key = action_rc.clone();

        let int_hover = interaction.clone();
        let int_tap = interaction.clone();
        let self_id = ctx.self_id();

        let mut handler_set = HandlerSet::new()
            .on_tap({
                move |ctx: &mut EventContext| {
                    if !enabled {
                        return;
                    }
                    int_tap.set(MenuItemState::Pressed);
                    if let Some(ref action) = *action_for_tap {
                        action(ctx);
                    }
                }
            })
            .on_hover({
                let submenu_id = submenu_content_id;
                move |entered: bool, ctx: &mut EventContext| {
                    if !enabled {
                        return;
                    }
                    if entered {
                        int_hover.set(MenuItemState::Hovered);
                        // Open submenu on hover
                        if let Some(sub_id) = submenu_id {
                            ctx.activate(sub_id);
                            ctx.show_overlay(OverlayRequest {
                                content_id: sub_id,
                                anchor: self_id,
                                placement: OverlayPlacement::TrailingEdge,
                                dismiss: DismissBehavior::ClickOutside,
                                layer: OverlayLayer::InTree,
                                parent_overlay: None,
                            });
                        }
                    } else {
                        int_hover.set(MenuItemState::Idle);
                    }
                }
            })
            .on_key({
                let interaction = interaction.clone();
                move |event: &WidgetEvent, ctx: &mut EventContext| -> EventResponse {
                    if !enabled {
                        return EventResponse::Ignored;
                    }
                    match event {
                        WidgetEvent::KeyDown {
                            key: Key::Enter | Key::Space,
                            ..
                        } => {
                            if let Some(ref action) = *action_for_key {
                                action(ctx);
                            }
                            interaction.set(MenuItemState::Pressed);
                            EventResponse::Handled
                        }
                        _ => EventResponse::Ignored,
                    }
                }
            });

        if enabled {
            handler_set = handler_set.cursor(CursorIcon::Pointer);
        }

        ctx.apply_self_handlers(handler_set);

        vec![root_id]
    }

    fn size_that_fits(&self, proposal: SizeProposal, ctx: &LayoutContext) -> Size {
        match self.root_child_id {
            Some(id) => {
                let size = ctx
                    .child_size(id, proposal)
                    .unwrap_or_else(|| proposal.resolve(0.0, 0.0));
                // Enforce minimum height of 32px for touch targets
                Size::new(size.width, size.height.max(32.0))
            }
            None => proposal.resolve(120.0, 32.0),
        }
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
        builder.set_role(fern_core::accesskit::Role::MenuItem);
        builder.set_name(&self.label);
        if !self.enabled {
            builder.set_disabled();
        }
        builder.add_action(fern_core::accesskit::Action::Click);
    }

    fn children(&self) -> Vec<WidgetId> {
        match self.root_child_id {
            Some(id) => vec![id],
            None => Vec::new(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use fern_canvas::Point;
    use fern_core::app_command::AppCommand;
    use fern_core::event::PointerButton;
    use fern_core::widget_tree::WidgetTree;
    use fern_tokens::Theme;
    use std::cell::Cell;
    use std::rc::Rc;

    #[derive(Debug, Clone, PartialEq)]
    enum TestCmd {
        Cut,
        Paste,
    }
    impl AppCommand for TestCmd {}

    fn setup_item(label: &str, cmd: TestCmd) -> (WidgetTree, WidgetId) {
        let mut tree = WidgetTree::new().with_theme(Theme::light_default());
        let item = tree.add(MenuItem::new(label).on_activate(cmd));
        tree.layout(SizeProposal::exact(200.0, 40.0));
        (tree, item)
    }

    #[test]
    fn tap_emits_command() {
        let (mut tree, item) = setup_item("Cut", TestCmd::Cut);
        let called = Rc::new(Cell::new(false));
        let c = called.clone();
        tree.on_command(move |cmd: &TestCmd| {
            if *cmd == TestCmd::Cut {
                c.set(true);
            }
        });
        tree.click(item);
        assert!(called.get());
    }

    #[test]
    fn disabled_ignores_tap() {
        let mut tree = WidgetTree::new().with_theme(Theme::light_default());
        let item = tree.add(MenuItem::new("Cut").on_activate(TestCmd::Cut).enabled(false));
        tree.layout(SizeProposal::exact(200.0, 40.0));
        let called = Rc::new(Cell::new(false));
        let c = called.clone();
        tree.on_command(move |_cmd: &TestCmd| c.set(true));
        tree.click(item);
        assert!(!called.get());
    }

    #[test]
    fn accessibility_role() {
        let (tree, item) = setup_item("Cut", TestCmd::Cut);
        let info = tree.accessibility_node(item);
        assert_eq!(info.role(), fern_core::accesskit::Role::MenuItem);
        assert_eq!(info.name(), Some("Cut"));
    }

    #[test]
    fn minimum_height() {
        let (tree, item) = setup_item("X", TestCmd::Cut);
        let bounds = tree.bounds(item);
        assert!(bounds.height >= 32.0);
    }

    #[test]
    fn with_shortcut_label() {
        let mut tree = WidgetTree::new().with_theme(Theme::light_default());
        let item = tree.add(
            MenuItem::new("Cut")
                .on_activate(TestCmd::Cut)
                .shortcut_label("Ctrl+X"),
        );
        tree.layout(SizeProposal::exact(300.0, 40.0));
        // Just verify it builds and lays out without panic
        assert!(tree.bounds(item).width > 0.0);
    }

    #[test]
    fn submenu_item_has_chevron() {
        let mut tree = WidgetTree::new().with_theme(Theme::light_default());
        let item = tree.add(MenuItem::submenu("Open Recent", || {
            Box::new(TextWidget::new("placeholder"))
        }));
        tree.layout(SizeProposal::exact(300.0, 40.0));
        // Verify it builds with the chevron without panic
        assert!(tree.bounds(item).width > 0.0);
    }
}
