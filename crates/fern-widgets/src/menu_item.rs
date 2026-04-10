//! MenuItem widget — a single item in a menu or context menu.
//!
//! Non-generic, closure-based command erasure (same pattern as Button).
//! Supports icons, shortcut labels, disabled state, and submenu triggers.

use std::time::Duration;

use fern_canvas::{Rect, Size, SizeProposal};
use fern_core::accessibility::AccessNodeBuilder;
use fern_core::app_command::AppCommand;
use fern_core::build_context::BuildContext;
use fern_core::event::{EventResponse, Key, WidgetEvent};
use fern_core::overlay::{DismissBehavior, OverlayLayer, OverlayPlacement, OverlayRequest};
use fern_core::signal::Signal;
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

/// Default delay before a submenu opens on hover (200ms).
/// This delay also provides diagonal movement tolerance: when the pointer
/// crosses other menu items while moving toward a submenu, those items
/// don't open their submenus because the delay hasn't elapsed yet.
const DEFAULT_SUBMENU_OPEN_DELAY: Duration = Duration::from_millis(200);
const DEFAULT_SUBMENU_CLOSE_DELAY: Duration = Duration::from_millis(150);

/// A single menu item: icon + label + shortcut label + optional submenu chevron.
pub struct MenuItem {
    label: String,
    icon: Option<IconWidget>,
    shortcut_label: Option<String>,
    action: Option<CommandFactory>,
    /// Type-erased command for automatic shortcut label lookup via ShortcutMap.
    command_any: Option<Box<dyn std::any::Any>>,
    enabled: bool,
    submenu_factory: Option<Box<dyn Fn() -> Box<dyn Widget>>>,
    submenu_open_delay: Duration,
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
            command_any: None,
            enabled: true,
            submenu_factory: None,
            submenu_open_delay: DEFAULT_SUBMENU_OPEN_DELAY,
            interaction: Signal::new(MenuItemState::Idle),
            root_child_id: None,
            submenu_content_id: None,
        }
    }

    /// Set the command to emit on activation. Generic only at this call site.
    /// Also stores the command for automatic shortcut label lookup from the ShortcutMap.
    pub fn on_activate<C: AppCommand>(mut self, command: C) -> Self {
        let cmd_for_lookup = command.clone();
        self.action = Some(Box::new(move |ctx: &mut EventContext| {
            ctx.emit(command.clone());
        }));
        self.command_any = Some(Box::new(cmd_for_lookup));
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

    /// Create a submenu trigger item. The factory is invoked during `build()` to
    /// pre-create the submenu content (typically a `MenuList`), which is kept
    /// dormant until the hover delay elapses.
    pub fn submenu(
        label: impl Into<String>,
        factory: impl Fn() -> Box<dyn Widget> + 'static,
    ) -> Self {
        Self {
            label: label.into(),
            icon: None,
            shortcut_label: None,
            action: None,
            command_any: None,
            enabled: true,
            submenu_factory: Some(Box::new(factory)),
            submenu_open_delay: DEFAULT_SUBMENU_OPEN_DELAY,
            interaction: Signal::new(MenuItemState::Idle),
            root_child_id: None,
            submenu_content_id: None,
        }
    }

    /// Set a custom submenu open delay (default: 200ms).
    pub fn submenu_delay(mut self, delay: Duration) -> Self {
        self.submenu_open_delay = delay;
        self
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

        // Shortcut label (dimmed) — manual label takes precedence, then auto-lookup from ShortcutMap
        let resolved_shortcut = self.shortcut_label.clone().or_else(|| {
            self.command_any
                .as_ref()
                .and_then(|cmd| ctx.shortcut_label_for_any(cmd.as_ref()))
        });
        if let Some(ref shortcut_text) = resolved_shortcut {
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
        let action_for_key = action_rc.clone();

        let int_hover = interaction.clone();
        let self_id = ctx.self_id();
        let is_submenu = submenu_content_id.is_some();

        let mut handler_set = HandlerSet::new();

        if is_submenu {
            // --- Submenu trigger: timer-based delayed open ---
            // On hover enter: request a delayed overlay via the widget tree's
            // timer system (like tooltips). On hover leave: cancel the pending
            // request. The widget tree checks pending overlays during layout()
            // and opens them once the delay elapses.
            let sub_id = submenu_content_id.unwrap();
            let open_delay = self.submenu_open_delay;

            handler_set = handler_set
                .on_tap({
                    let int_hover = int_hover.clone();
                    move |ctx: &mut EventContext| {
                        if !enabled {
                            return;
                        }
                        // Click on submenu trigger opens it immediately
                        ctx.dismiss_child_overlays_except(sub_id);
                        ctx.activate(sub_id);
                        ctx.show_overlay(OverlayRequest {
                            content_id: sub_id,
                            anchor: self_id,
                            placement: OverlayPlacement::TrailingEdge,
                            dismiss: DismissBehavior::PointerLeave {
                                delay: DEFAULT_SUBMENU_CLOSE_DELAY,
                            },
                            layer: OverlayLayer::InTree,
                            parent_overlay: None,
                        });
                        ctx.request_focus(sub_id);
                    }
                })
                .on_hover({
                    let int_hover = int_hover.clone();
                    move |entered: bool, ctx: &mut EventContext| {
                        if !enabled {
                            return;
                        }
                        if entered {
                            int_hover.set(MenuItemState::Hovered);
                            ctx.dismiss_child_overlays_except(sub_id);
                            ctx.show_overlay_after_with_focus(
                                OverlayRequest {
                                    content_id: sub_id,
                                    anchor: self_id,
                                    placement: OverlayPlacement::TrailingEdge,
                                    dismiss: DismissBehavior::PointerLeave {
                                        delay: DEFAULT_SUBMENU_CLOSE_DELAY,
                                    },
                                    layer: OverlayLayer::InTree,
                                    parent_overlay: None,
                                },
                                open_delay,
                                sub_id,
                            );
                        } else {
                            int_hover.set(MenuItemState::Idle);
                            ctx.cancel_delayed_overlay(sub_id);
                        }
                    }
                });
        } else {
            // --- Regular menu item: tap to activate ---
            let action_for_tap = action_rc.clone();
            let int_tap = interaction.clone();

            handler_set = handler_set
                .on_tap({
                    move |ctx: &mut EventContext| {
                        if !enabled {
                            return;
                        }
                        int_tap.set(MenuItemState::Pressed);
                        if let Some(ref action) = *action_for_tap {
                            action(ctx);
                            ctx.dismiss_all_overlays();
                        }
                    }
                })
                .on_hover({
                    move |entered: bool, ctx: &mut EventContext| {
                        if !enabled {
                            return;
                        }
                        if entered {
                            ctx.dismiss_child_overlays();
                            int_hover.set(MenuItemState::Hovered);
                        } else {
                            int_hover.set(MenuItemState::Idle);
                        }
                    }
                });
        }

        // Keyboard handler shared by both submenu and regular items
        handler_set = handler_set.on_key({
            let interaction = interaction.clone();
            let sub_id = submenu_content_id;
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
                            ctx.dismiss_all_overlays();
                        } else if let Some(sub_id) = sub_id {
                            ctx.dismiss_child_overlays_except(sub_id);
                            ctx.activate(sub_id);
                            ctx.show_overlay(OverlayRequest {
                                content_id: sub_id,
                                anchor: self_id,
                                placement: OverlayPlacement::TrailingEdge,
                                dismiss: DismissBehavior::PointerLeave {
                                    delay: DEFAULT_SUBMENU_CLOSE_DELAY,
                                },
                                layer: OverlayLayer::InTree,
                                parent_overlay: None,
                            });
                            ctx.request_focus(sub_id);
                        }
                        interaction.set(MenuItemState::Pressed);
                        EventResponse::Handled
                    }
                    // ArrowRight opens submenu (ignored on regular items)
                    WidgetEvent::KeyDown {
                        key: Key::ArrowRight,
                        ..
                    } => {
                        if let Some(sub_id) = sub_id {
                            ctx.dismiss_child_overlays_except(sub_id);
                            ctx.activate(sub_id);
                            ctx.show_overlay(OverlayRequest {
                                content_id: sub_id,
                                anchor: self_id,
                                placement: OverlayPlacement::TrailingEdge,
                                dismiss: DismissBehavior::PointerLeave {
                                    delay: DEFAULT_SUBMENU_CLOSE_DELAY,
                                },
                                layer: OverlayLayer::InTree,
                                parent_overlay: None,
                            });
                            ctx.request_focus(sub_id);
                            EventResponse::Handled
                        } else {
                            EventResponse::Ignored
                        }
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
    use fern_core::widget_tree::WidgetTree;
    use fern_tokens::Theme;
    use std::cell::Cell;
    use std::cell::RefCell;
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

    fn capture_commands(tree: &mut WidgetTree) -> Rc<RefCell<Vec<TestCmd>>> {
        let commands = Rc::new(RefCell::new(Vec::new()));
        let captured = commands.clone();
        tree.on_command(move |cmd: &TestCmd| {
            captured.borrow_mut().push(cmd.clone());
        });
        commands
    }

    fn collect_descendants(tree: &WidgetTree, root: WidgetId, out: &mut Vec<WidgetId>) {
        out.push(root);
        for child in tree.children(root) {
            collect_descendants(tree, child, out);
        }
    }

    fn descendants(tree: &WidgetTree, root: WidgetId) -> Vec<WidgetId> {
        let mut out = Vec::new();
        collect_descendants(tree, root, &mut out);
        out
    }

    fn find_menu_item(tree: &WidgetTree, root: WidgetId, label: &str) -> WidgetId {
        descendants(tree, root)
            .into_iter()
            .find(|&id| {
                let info = tree.accessibility_node(id);
                info.role() == fern_core::accesskit::Role::MenuItem && info.name() == Some(label)
            })
            .unwrap_or_else(|| panic!("menu item '{label}' not found"))
    }

    fn overlay_contains_label(tree: &WidgetTree, label: &str) -> bool {
        tree.overlay_manager().active_content_ids().into_iter().any(|root| {
            descendants(tree, root).into_iter().any(|id| {
                let info = tree.accessibility_node(id);
                info.name() == Some(label)
            })
        })
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
    fn auto_shortcut_label_from_shortcut_map() {
        use fern_core::shortcut::{Shortcut, ShortcutMap};

        let shortcuts =
            ShortcutMap::new().bind(Shortcut::ctrl(Key::X), TestCmd::Cut);

        // Item with auto-resolved shortcut (via ShortcutMap)
        let mut tree_with = WidgetTree::new()
            .with_theme(Theme::light_default())
            .with_shortcuts(shortcuts);
        let item_with = tree_with.add(MenuItem::new("Cut").on_activate(TestCmd::Cut));
        tree_with.layout(SizeProposal::unspecified());

        // Item without shortcuts registered
        let mut tree_without = WidgetTree::new().with_theme(Theme::light_default());
        let item_without = tree_without.add(MenuItem::new("Cut").on_activate(TestCmd::Cut));
        tree_without.layout(SizeProposal::unspecified());

        // The item with an auto-resolved shortcut label should be wider
        let width_with = tree_with.bounds(item_with).width;
        let width_without = tree_without.bounds(item_without).width;
        assert!(
            width_with > width_without,
            "auto shortcut label should make item wider: {} vs {}",
            width_with,
            width_without
        );
    }

    #[test]
    fn manual_shortcut_label_overrides_auto() {
        use fern_core::shortcut::{Shortcut, ShortcutMap};

        let shortcuts =
            ShortcutMap::new().bind(Shortcut::ctrl(Key::X), TestCmd::Cut);

        // Manual label should take precedence over auto-lookup
        let mut tree = WidgetTree::new()
            .with_theme(Theme::light_default())
            .with_shortcuts(shortcuts);
        let item = tree.add(
            MenuItem::new("Cut")
                .on_activate(TestCmd::Cut)
                .shortcut_label("Custom"),
        );
        tree.layout(SizeProposal::exact(300.0, 40.0));
        // Should build without panic — manual label used
        assert!(tree.bounds(item).width > 0.0);
    }

    #[test]
    fn no_shortcut_label_when_command_not_bound() {
        use fern_core::shortcut::{Shortcut, ShortcutMap};

        // Only Paste is bound, not Cut
        let shortcuts =
            ShortcutMap::new().bind(Shortcut::ctrl(Key::V), TestCmd::Paste);

        let mut tree_with_map = WidgetTree::new()
            .with_theme(Theme::light_default())
            .with_shortcuts(shortcuts);
        let item_with_map =
            tree_with_map.add(MenuItem::new("Cut").on_activate(TestCmd::Cut));
        tree_with_map.layout(SizeProposal::unspecified());

        let mut tree_no_map = WidgetTree::new().with_theme(Theme::light_default());
        let item_no_map = tree_no_map.add(MenuItem::new("Cut").on_activate(TestCmd::Cut));
        tree_no_map.layout(SizeProposal::unspecified());

        // Widths should be the same — no shortcut label resolved for Cut
        let width_with = tree_with_map.bounds(item_with_map).width;
        let width_without = tree_no_map.bounds(item_no_map).width;
        assert!(
            (width_with - width_without).abs() < 0.01,
            "unbound command should produce no shortcut label: {} vs {}",
            width_with,
            width_without
        );
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

    #[test]
    fn submenu_does_not_open_immediately_on_hover() {
        let mut tree = WidgetTree::new().with_theme(Theme::light_default());
        let item = tree.add(MenuItem::submenu("More", || {
            Box::new(
                crate::menu_list::MenuList::new()
                    .item(MenuItem::new("Sub").on_activate(TestCmd::Cut)),
            )
        }));
        tree.layout(SizeProposal::exact(200.0, 40.0));

        assert!(tree.active_overlays().is_empty());

        // Hover over the item — submenu should NOT open immediately
        let center = tree.bounds(item).center();
        tree.pointer_move(center);

        // Advance just a tiny amount — not enough for the 200ms default delay
        tree.advance_time(std::time::Duration::from_millis(50));

        assert!(
            tree.active_overlays().is_empty(),
            "submenu should not open immediately on hover (delay required)"
        );
    }

    #[test]
    fn submenu_opens_after_delay() {
        let mut tree = WidgetTree::new().with_theme(Theme::light_default());
        let commands = capture_commands(&mut tree);
        let item = tree.add(
            MenuItem::submenu("More", || {
                Box::new(
                    crate::menu_list::MenuList::new()
                        .item(MenuItem::new("Sub").on_activate(TestCmd::Cut)),
                )
            })
            .submenu_delay(std::time::Duration::from_millis(100)),
        );
        tree.layout(SizeProposal::exact(200.0, 40.0));

        assert!(tree.active_overlays().is_empty());

        // Hover over the submenu trigger
        let center = tree.bounds(item).center();
        tree.pointer_move(center);

        // Advance past the delay
        tree.advance_time(std::time::Duration::from_millis(150));

        assert_eq!(
            tree.active_overlays().len(),
            1,
            "submenu should open after delay elapses"
        );

        tree.press_key(Key::ArrowDown, fern_core::event::Modifiers::NONE);
        tree.press_key(Key::Enter, fern_core::event::Modifiers::NONE);
        tree.layout(SizeProposal::exact(200.0, 40.0));

        assert_eq!(&*commands.borrow(), &[TestCmd::Cut]);
    }

    #[test]
    fn submenu_delay_provides_diagonal_tolerance() {
        // With a non-zero delay, quickly moving through a submenu trigger
        // to another item cancels the pending open — this IS diagonal tolerance.
        let mut tree = WidgetTree::new().with_theme(Theme::light_default());
        let item = tree.add(MenuItem::submenu("More", || {
            Box::new(
                crate::menu_list::MenuList::new()
                    .item(MenuItem::new("Sub").on_activate(TestCmd::Cut)),
            )
        }));
        tree.layout(SizeProposal::exact(200.0, 80.0));

        // Move pointer into the submenu trigger briefly
        let center = tree.bounds(item).center();
        tree.pointer_move(center);
        // Immediately move away (simulating diagonal movement) — cancels the pending open
        tree.pointer_move(Point::new(center.x, center.y + 50.0));

        // Even after the delay elapses, the submenu should NOT open
        tree.advance_time(std::time::Duration::from_millis(300));

        assert!(
            tree.active_overlays().is_empty(),
            "quick pass-through should not open submenu (diagonal tolerance)"
        );
    }

    #[test]
    fn submenu_opens_on_enter_key() {
        let mut tree = WidgetTree::new().with_theme(Theme::light_default());
        let commands = capture_commands(&mut tree);
        let item = tree.add(MenuItem::submenu("More", || {
            Box::new(
                crate::menu_list::MenuList::new()
                    .item(MenuItem::new("Sub").on_activate(TestCmd::Cut)),
            )
        }));
        tree.layout(SizeProposal::exact(200.0, 40.0));
        tree.focus(item);

        assert!(tree.active_overlays().is_empty());

        // Enter key should open submenu immediately (no delay for keyboard)
        tree.press_key(Key::Enter, fern_core::event::Modifiers::NONE);
        tree.layout(SizeProposal::exact(200.0, 40.0));

        assert_eq!(
            tree.active_overlays().len(),
            1,
            "Enter key should open submenu immediately"
        );

        tree.press_key(Key::ArrowDown, fern_core::event::Modifiers::NONE);
        tree.press_key(Key::Enter, fern_core::event::Modifiers::NONE);
        tree.layout(SizeProposal::exact(200.0, 40.0));

        assert_eq!(&*commands.borrow(), &[TestCmd::Cut]);
    }

    #[test]
    fn hovering_regular_sibling_closes_open_submenu() {
        let mut tree = WidgetTree::new().with_theme(Theme::light_default());
        let menu = tree.add(
            crate::menu_list::MenuList::new()
                .item(MenuItem::submenu("More", || {
                    Box::new(
                        crate::menu_list::MenuList::new()
                            .item(MenuItem::new("Sub").on_activate(TestCmd::Cut)),
                    )
                }))
                .item(MenuItem::new("Paste").on_activate(TestCmd::Paste)),
        );
        tree.layout(SizeProposal::exact(240.0, 120.0));

        let submenu_item = find_menu_item(&tree, menu, "More");
        let regular_item = find_menu_item(&tree, menu, "Paste");

        tree.pointer_move(tree.bounds(submenu_item).center());
        tree.advance_time(std::time::Duration::from_millis(250));
        assert_eq!(tree.active_overlays().len(), 1);
        assert!(overlay_contains_label(&tree, "Sub"));

        tree.pointer_move(tree.bounds(regular_item).center());

        assert!(tree.active_overlays().is_empty());
    }

    #[test]
    fn hovering_sibling_submenu_replaces_previous_branch() {
        let mut tree = WidgetTree::new().with_theme(Theme::light_default());
        let menu = tree.add(
            crate::menu_list::MenuList::new()
                .item(MenuItem::submenu("More", || {
                    Box::new(
                        crate::menu_list::MenuList::new()
                            .item(MenuItem::new("Sub A").on_activate(TestCmd::Cut)),
                    )
                }))
                .item(MenuItem::submenu("Recent", || {
                    Box::new(
                        crate::menu_list::MenuList::new()
                            .item(MenuItem::new("Sub B").on_activate(TestCmd::Paste)),
                    )
                })),
        );
        tree.layout(SizeProposal::exact(240.0, 120.0));

        let first = find_menu_item(&tree, menu, "More");
        let second = find_menu_item(&tree, menu, "Recent");

        tree.pointer_move(tree.bounds(first).center());
        tree.advance_time(std::time::Duration::from_millis(250));
        assert_eq!(tree.active_overlays().len(), 1);
        assert!(overlay_contains_label(&tree, "Sub A"));

        tree.pointer_move(tree.bounds(second).center());
        assert!(tree.active_overlays().is_empty());

        tree.advance_time(std::time::Duration::from_millis(250));
        assert_eq!(tree.active_overlays().len(), 1);
        assert!(overlay_contains_label(&tree, "Sub B"));
        assert!(!overlay_contains_label(&tree, "Sub A"));
    }

    #[test]
    fn moving_pointer_outside_closes_open_submenu_after_delay() {
        let mut tree = WidgetTree::new().with_theme(Theme::light_default());
        let menu = tree.add(
            crate::menu_list::MenuList::new().item(MenuItem::submenu("More", || {
                Box::new(
                    crate::menu_list::MenuList::new()
                        .item(MenuItem::new("Sub").on_activate(TestCmd::Cut)),
                )
            })),
        );
        tree.layout(SizeProposal::exact(240.0, 80.0));

        let submenu_item = find_menu_item(&tree, menu, "More");
        tree.pointer_move(tree.bounds(submenu_item).center());
        tree.advance_time(std::time::Duration::from_millis(250));
        assert_eq!(tree.active_overlays().len(), 1);

        tree.pointer_move(Point::new(1000.0, 1000.0));
        tree.advance_time(std::time::Duration::from_millis(100));
        assert_eq!(tree.active_overlays().len(), 1);

        tree.advance_time(std::time::Duration::from_millis(100));
        assert!(tree.active_overlays().is_empty());
    }

    #[test]
    fn custom_submenu_delay() {
        // Verify the submenu_delay builder method is accepted
        let mut tree = WidgetTree::new().with_theme(Theme::light_default());
        let item = tree.add(
            MenuItem::submenu("More", || {
                Box::new(TextWidget::new("placeholder"))
            })
            .submenu_delay(std::time::Duration::from_millis(500)),
        );
        tree.layout(SizeProposal::exact(200.0, 40.0));
        assert!(tree.bounds(item).width > 0.0);
    }
}
