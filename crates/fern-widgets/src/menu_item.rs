//! MenuItem widget — a single item in a menu or context menu.
//!
//! Non-generic, closure-based command erasure (same pattern as Button).
//! Supports icons, shortcut labels, disabled state, and submenu triggers.

use std::rc::Rc;
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

/// Type-erased command factory. Stored as `Rc` (not `Box`) so the closure
/// can be cloned and shared — in particular with SplitButton, which reads
/// the action out of a MenuItem via `MenuItem::action()` and re-fires it
/// from its main region without disturbing the MenuItem's own use of it.
type CommandFactory = Rc<dyn Fn(&mut EventContext)>;

/// Interaction state for a menu item.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum MenuItemState {
    Idle,
    Hovered,
    Pressed,
    Disabled,
}

/// Default delay before a submenu opens on hover (400 ms — IntelliJ's value).
/// This delay also provides diagonal movement tolerance: when the pointer
/// crosses other menu items while moving toward a submenu, those items
/// don't open their submenus because the delay hasn't elapsed yet. 400 ms
/// is long enough that a casual sweep past a submenu trigger doesn't
/// accidentally open it, but short enough that a deliberate hover feels
/// responsive.
const DEFAULT_SUBMENU_OPEN_DELAY: Duration = Duration::from_millis(400);
const DEFAULT_SUBMENU_CLOSE_DELAY: Duration = Duration::from_millis(150);

/// A single menu item: icon + label + shortcut label + optional submenu chevron.
pub struct MenuItem {
    label: String,
    icon: Option<IconWidget>,
    shortcut_label: Option<String>,
    tooltip_text: Option<String>,
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
    pub fn new(label: impl Into<fern_i18n::LocalizedString>) -> Self {
        let ls: fern_i18n::LocalizedString = label.into();
        Self {
            label: ls.resolve_now(),
            icon: None,
            shortcut_label: None,
            tooltip_text: None,
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

    /// Shim (permanent, `#[doc(hidden)]`) — wraps a raw label in `LocalizedString::literal`.
    #[doc(hidden)]
    pub fn new_literal(label: impl Into<String>) -> Self {
        Self::new(fern_i18n::LocalizedString::literal(label))
    }

    /// Set the command to emit on activation. Generic only at this call site.
    /// Also stores the command for automatic shortcut label lookup from the ShortcutMap.
    pub fn on_activate<C: AppCommand>(mut self, command: C) -> Self {
        let cmd_for_lookup = command.clone();
        self.action = Some(Rc::new(move |ctx: &mut EventContext| {
            ctx.emit(command.clone());
        }));
        self.command_any = Some(Box::new(cmd_for_lookup));
        self
    }

    /// Escape hatch: arbitrary closure invoked on activation.
    /// See architecture Section 9.2.6.
    /// Note: shortcut label auto-lookup is not available with this variant
    /// since there is no typed command to look up.
    pub fn on_activate_fn(mut self, f: impl Fn(&mut EventContext) + 'static) -> Self {
        self.action = Some(Rc::new(f));
        self
    }

    /// Read the item's display label. Exposed so SplitButton (and any other
    /// compound widget that embeds a MenuItem) can mirror the label in its
    /// own chrome.
    pub fn label(&self) -> &str {
        &self.label
    }

    /// Clone out a shared handle to the activation closure. Returns `None`
    /// when this MenuItem has no action (e.g. it's a submenu trigger). The
    /// returned `Rc` aliases MenuItem's own internal handle — invoking it
    /// has the same effect as the user clicking this menu item (minus the
    /// overlay dismissal that the tap handler also performs).
    pub fn action(&self) -> Option<Rc<dyn Fn(&mut EventContext)>> {
        self.action.clone()
    }

    /// Set a leading icon.
    pub fn icon(mut self, icon: IconWidget) -> Self {
        self.icon = Some(icon);
        self
    }

    /// Set a trailing shortcut label (e.g., "Ctrl+X"). Shortcut labels are
    /// typically not translated (they're the key combination literal), so
    /// this accepts a plain string.
    pub fn shortcut_label(mut self, label: impl Into<String>) -> Self {
        self.shortcut_label = Some(label.into());
        self
    }

    pub fn enabled(mut self, enabled: bool) -> Self {
        self.enabled = enabled;
        self
    }

    /// Attach a tooltip that appears after a hover delay, same mechanism
    /// as [`Button::tooltip`](crate::button::Button::tooltip).
    pub fn tooltip(mut self, text: impl Into<fern_i18n::LocalizedString>) -> Self {
        let ls: fern_i18n::LocalizedString = text.into();
        self.tooltip_text = Some(ls.resolve_now());
        self
    }

    /// Shim (permanent, `#[doc(hidden)]`) for `tooltip(...)` accepting a raw string.
    #[doc(hidden)]
    pub fn tooltip_literal(mut self, text: impl Into<String>) -> Self {
        self.tooltip_text = Some(text.into());
        self
    }

    /// Create a submenu trigger item. The factory is invoked during `build()` to
    /// pre-create the submenu content (typically a `MenuList`), which is kept
    /// dormant until the hover delay elapses.
    pub fn submenu(
        label: impl Into<fern_i18n::LocalizedString>,
        factory: impl Fn() -> Box<dyn Widget> + 'static,
    ) -> Self {
        let ls: fern_i18n::LocalizedString = label.into();
        Self {
            label: ls.resolve_now(),
            icon: None,
            shortcut_label: None,
            tooltip_text: None,
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

    /// Shim (permanent, `#[doc(hidden)]`) for `submenu(...)` accepting a raw label.
    #[doc(hidden)]
    pub fn submenu_literal(
        label: impl Into<String>,
        factory: impl Fn() -> Box<dyn Widget> + 'static,
    ) -> Self {
        Self::submenu(fern_i18n::LocalizedString::literal(label), factory)
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

fn resolve_bg(state: MenuItemState, hover: Color, pressed: Color) -> Color {
    match state {
        MenuItemState::Idle | MenuItemState::Disabled => Color::TRANSPARENT,
        MenuItemState::Hovered => hover,
        MenuItemState::Pressed => pressed,
    }
}

fn resolve_text(state: MenuItemState, text_color: Color, disabled_color: Color) -> Color {
    match state {
        MenuItemState::Disabled => disabled_color,
        _ => text_color,
    }
}

fn resolve_shortcut(
    state: MenuItemState,
    shortcut_color: Color,
    disabled_color: Color,
) -> Color {
    match state {
        MenuItemState::Disabled => disabled_color,
        _ => shortcut_color,
    }
}

impl Widget for MenuItem {
    fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
        let theme = ctx.theme().clone();
        let menu_style = theme.components.menu;
        let enabled = self.enabled;

        let interaction = ctx.signal(if enabled {
            MenuItemState::Idle
        } else {
            MenuItemState::Disabled
        });
        self.interaction = interaction.clone();

        // Background: use the Int UI surface_hover / surface_pressed tokens
        // directly instead of a hand-mixed alpha wash. Tracks theme changes.
        let bg_color = {
            let hover = theme.colors.surface_hover;
            let pressed = theme.colors.surface_pressed;
            interaction.map(move |s| resolve_bg(*s, hover, pressed))
        };

        let text_color = {
            let text = theme.colors.text_primary;
            let disabled = theme.colors.text_disabled;
            interaction.map(move |s| resolve_text(*s, text, disabled))
        };

        // Row layout:
        //   [icon column][gap][label][Spacer][shortcut?][chevron column]
        //
        // HStack spacing is 0 — we insert an explicit `icon_label_gap`
        // only between the icon column and the label. Nothing else in the
        // row should have inter-child gaps: the Spacer handles stretch,
        // the chevron column handles the trailing padding, and the
        // shortcut (when present) sits directly adjacent to the chevron
        // column. Using HStack::spacing here would inject extra gaps
        // around the Spacer and shortcut, pushing the shortcut visibly
        // away from the trailing edge — which is why "Ctrl+X" used to
        // land short of where regular items had their right padding.
        //
        // * `icon column` is always reserved at `icon_column_width`, even
        //   when the item has no icon, so labels line up vertically
        //   between icon'd and icon-less items.
        //
        // * `chevron column` is always reserved at `item_padding_horizontal`
        //   width. For submenu items it contains the chevron; for regular
        //   items it's empty. Because the outer wrapper sets right
        //   padding = 0, the chevron column visually IS the right
        //   padding — regular items and submenu items share the same
        //   trailing edge.
        let mut row = HStack::new().spacing(0.0);

        // Icon column — fixed width, optional IconWidget inside.
        let icon_child_id = if let Some(icon) = self.icon.take() {
            ctx.add(icon.bind_color(text_color.clone()))
        } else {
            ctx.add(Spacer::new())
        };
        let icon_column = ctx.add(
            crate::primitives::FixedSize::new()
                .bind_width(menu_style.icon_column_width)
                .bind_height(menu_style.icon_column_width)
                .set_child(icon_child_id),
        );
        row = row.add_child(icon_column);

        // Explicit icon-to-label gap (rendered as a fixed-width Spacer
        // rather than HStack::spacing to avoid injecting gaps around the
        // other children).
        let icon_label_spacer = ctx.add(Spacer::new());
        let icon_label_gap = ctx.add(
            crate::primitives::FixedSize::new()
                .bind_width(menu_style.icon_label_gap)
                .bind_height(1.0_f32)
                .set_child(icon_label_spacer),
        );
        row = row.add_child(icon_label_gap);

        // Label
        let label = TextWidget::new_literal(&self.label)
            .style(theme.typography.body.clone())
            .bind_color(text_color.clone())
            .single_line();
        row = row.child(label);

        // Stretch spacer — pushes trailing content to the right edge.
        row = row.child(Spacer::new());

        // Shortcut label — manual label takes precedence, then auto-lookup
        // from ShortcutMap. Uses the dedicated `tooltip_shortcut` color
        // token at the same size as the body label.
        //
        // A fixed-width gap (`shortcut_left_gap`, 24 dp) is inserted
        // between the stretch Spacer and the shortcut label so that even
        // when the row is packed tight (Spacer stretch = 0), there is
        // always a visible gap between label and shortcut. This mirrors
        // the `icon_label_gap` pattern: a FixedSize-wrapped Spacer acts
        // as a fixed non-spacer child so HStack can't collapse it.
        let resolved_shortcut = self.shortcut_label.clone().or_else(|| {
            self.command_any
                .as_ref()
                .and_then(|cmd| ctx.shortcut_label_for_any(cmd.as_ref()))
        });
        if let Some(ref shortcut_text) = resolved_shortcut {
            // Fixed minimum gap, always present.
            let shortcut_gap_spacer = ctx.add(Spacer::new());
            let shortcut_gap = ctx.add(
                crate::primitives::FixedSize::new()
                    .bind_width(menu_style.shortcut_left_gap)
                    .bind_height(1.0_f32)
                    .set_child(shortcut_gap_spacer),
            );
            row = row.add_child(shortcut_gap);

            let shortcut_color = {
                let shortcut = theme.colors.tooltip_shortcut;
                let disabled = theme.colors.text_disabled;
                interaction.map(move |s| resolve_shortcut(*s, shortcut, disabled))
            };
            let shortcut = TextWidget::new_literal(shortcut_text)
                .style(theme.typography.body.clone())
                .bind_color(shortcut_color)
                .single_line();
            row = row.child(shortcut);
        }

        // Pre-create submenu content if this is a submenu trigger. Kept
        // dormant until hover opens the overlay.
        let submenu_content_id = if let Some(factory) = self.submenu_factory.take() {
            let submenu_widget = factory();
            let id = ctx.add_boxed(submenu_widget);
            ctx.set_dormant(id);
            self.submenu_content_id = Some(id);
            Some(id)
        } else {
            None
        };

        // Chevron column — always reserved at `item_padding_horizontal`
        // width so submenu and regular items share the same trailing edge.
        let chevron_child_id = if submenu_content_id.is_some() {
            ctx.add(IconWidget::chevron_right(12.0).bind_color(text_color.clone()))
        } else {
            ctx.add(Spacer::new())
        };
        let chevron_column = ctx.add(
            crate::primitives::FixedSize::new()
                .bind_width(menu_style.item_padding_horizontal)
                .bind_height(menu_style.icon_column_width)
                .set_child(chevron_child_id),
        );
        row = row.add_child(chevron_column);

        let row_id = ctx.add(row);

        // Padding: vertical derived so the row has the full `item_height`
        // (24 dp); left padding uses `item_padding_horizontal`; RIGHT
        // padding is zero because the chevron column occupies that space.
        // Body text is 13 dp so that's ~5.5 dp top + 5.5 dp bottom.
        let pad_v = ((menu_style.item_height - theme.typography.body.size) * 0.5).max(0.0);
        let padding = Padding::new(
            pad_v,                              // top
            0.0,                                // right — chevron column fills this
            pad_v,                              // bottom
            menu_style.item_padding_horizontal, // left
        )
        .set_child(row_id);
        let padding_id = ctx.add(padding);

        // Background rect
        let rect = RectWidget::new().bind_background(bg_color);
        let rect_id = ctx.add(rect);

        let zstack = ZStack::new().add_child(rect_id).add_child(padding_id);
        let root_id = ctx.add(zstack);

        self.root_child_id = Some(root_id);

        // Attach tooltip if configured. Same 500ms delay as Button.
        if let Some(ref tooltip_text) = self.tooltip_text {
            let tooltip_widget = crate::tooltip::TooltipWidget::new_literal(tooltip_text);
            let tooltip_id = ctx.add(tooltip_widget);
            let delay = std::time::Duration::from_millis(500);
            ctx.attach_tooltip(root_id, tooltip_id, delay);
        }

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
                    move |_pos, ctx: &mut EventContext| {
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
                    move |_pos, ctx: &mut EventContext| {
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

        handler_set = handler_set.cursor(if enabled {
            CursorIcon::Pointer
        } else {
            CursorIcon::NotAllowed
        });

        ctx.apply_self_handlers(handler_set);

        vec![root_id]
    }

    fn size_that_fits(&self, proposal: SizeProposal, ctx: &LayoutContext) -> Size {
        match self.root_child_id {
            Some(id) => {
                let size = ctx
                    .child_size(id, proposal)
                    .unwrap_or_else(|| proposal.resolve(0.0, 0.0));
                // Claim the full proposed width when the parent offers one.
                // This is what makes menu items stretch to the popup width:
                // MenuList sizes its VStack to the widest item, then the
                // VStack proposes that width to each child. Without this
                // line, each MenuItem would report only its own content
                // width and the row's internal Spacer would have no room
                // to stretch — so the shortcut would sit flush against
                // the label instead of pushing to the trailing edge.
                let width = proposal.width.unwrap_or(size.width);
                // Enforce minimum height of 32px for touch targets
                Size::new(width, size.height.max(32.0))
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
        let item = tree.add(MenuItem::new_literal(label).on_activate(cmd));
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
        tree.overlay_manager()
            .active_content_ids()
            .into_iter()
            .any(|root| {
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
        let item = tree.add(
            MenuItem::new_literal("Cut")
                .on_activate(TestCmd::Cut)
                .enabled(false),
        );
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
            MenuItem::new_literal("Cut")
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

        let shortcuts = ShortcutMap::new().bind(Shortcut::ctrl(Key::X), TestCmd::Cut);

        // Item with auto-resolved shortcut (via ShortcutMap)
        let mut tree_with = WidgetTree::new()
            .with_theme(Theme::light_default())
            .with_shortcuts(shortcuts);
        let item_with = tree_with.add(MenuItem::new_literal("Cut").on_activate(TestCmd::Cut));
        tree_with.layout(SizeProposal::unspecified());

        // Item without shortcuts registered
        let mut tree_without = WidgetTree::new().with_theme(Theme::light_default());
        let item_without = tree_without.add(MenuItem::new_literal("Cut").on_activate(TestCmd::Cut));
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

        let shortcuts = ShortcutMap::new().bind(Shortcut::ctrl(Key::X), TestCmd::Cut);

        // Manual label should take precedence over auto-lookup
        let mut tree = WidgetTree::new()
            .with_theme(Theme::light_default())
            .with_shortcuts(shortcuts);
        let item = tree.add(
            MenuItem::new_literal("Cut")
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
        let shortcuts = ShortcutMap::new().bind(Shortcut::ctrl(Key::V), TestCmd::Paste);

        let mut tree_with_map = WidgetTree::new()
            .with_theme(Theme::light_default())
            .with_shortcuts(shortcuts);
        let item_with_map = tree_with_map.add(MenuItem::new_literal("Cut").on_activate(TestCmd::Cut));
        tree_with_map.layout(SizeProposal::unspecified());

        let mut tree_no_map = WidgetTree::new().with_theme(Theme::light_default());
        let item_no_map = tree_no_map.add(MenuItem::new_literal("Cut").on_activate(TestCmd::Cut));
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
        let item = tree.add(MenuItem::submenu_literal("Open Recent", || {
            Box::new(TextWidget::new_literal("placeholder"))
        }));
        tree.layout(SizeProposal::exact(300.0, 40.0));
        // Verify it builds with the chevron without panic
        assert!(tree.bounds(item).width > 0.0);
    }

    #[test]
    fn menu_item_stretches_to_proposed_width() {
        // Regression: the MenuItem row internally holds a Spacer between
        // label and shortcut which needs room to stretch so the shortcut
        // pushes to the trailing edge. Per-row stretching works only if
        // each MenuItem claims the full width proposed by the parent
        // (VStack inside MenuList), not just its content's intrinsic width.
        //
        // Without the `proposal.width.unwrap_or(...)` fix in
        // MenuItem::size_that_fits, this test fails because the item
        // collapses to its content width.
        let mut tree = WidgetTree::new().with_theme(Theme::light_default());
        let item = tree.add(
            MenuItem::new_literal("Cut")
                .on_activate(TestCmd::Cut)
                .shortcut_label("Ctrl+X"),
        );
        // Propose a wide container (300 dp) — well beyond the intrinsic
        // width of the "Cut" row (~100 dp with shortcut + columns).
        tree.layout(SizeProposal::exact(300.0, 40.0));
        let bounds = tree.bounds(item);
        assert!(
            (bounds.width - 300.0).abs() < 0.01,
            "MenuItem should claim the full proposed width (expected 300, got {})",
            bounds.width,
        );
    }

    #[test]
    fn shortcut_pushes_right_inside_menu_list() {
        // Reproduces actual usage: a MenuList containing multiple items
        // with different label lengths. The widest item determines the
        // popup width; narrower items should have their shortcut pushed
        // to the trailing edge with a visible gap between label and
        // shortcut.
        //
        // This catches the case that bare MenuItem tests miss: the extra
        // VStack / Padding / RectWidget layers between MenuList and the
        // individual items, which might break the width propagation.
        use crate::menu_list::MenuList;

        let mut tree = WidgetTree::new().with_theme(Theme::light_default());
        let menu = tree.add(
            MenuList::new()
                .item(
                    MenuItem::new_literal("Cut")
                        .on_activate(TestCmd::Cut)
                        .shortcut_label("Ctrl+X"),
                )
                .item(
                    MenuItem::new_literal("Paste With A Longer Label")
                        .on_activate(TestCmd::Paste)
                        .shortcut_label("Ctrl+V"),
                ),
        );
        tree.layout(SizeProposal::unspecified());

        // Walk MenuList's tree to find the MenuItem list:
        //   MenuList → ZStack → [RectWidget, Padding → VStack → [item1, item2, ...]]
        let menu_zstack = tree.child_widget(menu, 0);
        let menu_padding = tree.child_widget(menu_zstack, 1);
        let menu_vstack = tree.child_widget(menu_padding, 0);
        let items = tree.children(menu_vstack);
        assert_eq!(items.len(), 2, "expected 2 menu items");

        let menu_style = Theme::light_default().components.menu;

        // The first item ("Cut") is the narrow one. It should stretch to
        // the full popup width and its shortcut should be at the trailing
        // edge with a gap between "Cut" and "Ctrl+X".
        //
        // MenuList wraps each item in a KeyboardHighlightWrapper whose
        // structure is:  wrapper → ZStack → [bg_rect, MenuItem]
        // So we dive two levels to reach the MenuItem.
        let wrapper = items[0];
        let wrapper_zstack = tree.child_widget(wrapper, 0);
        let cut_item = tree.child_widget(wrapper_zstack, 1);
        let cut_bounds = tree.bounds(cut_item);

        let zstack = tree.child_widget(cut_item, 0);
        let padding = tree.child_widget(zstack, 1);
        let hstack = tree.child_widget(padding, 0);
        let row_children = tree.children(hstack);
        // Expected: [icon_col, icon_label_gap, label, Spacer,
        //            shortcut_gap, shortcut, chevron_col]
        assert_eq!(
            row_children.len(),
            7,
            "Cut row should have 7 children, got {}",
            row_children.len()
        );
        let label_tw = row_children[2];
        let shortcut_tw = row_children[5];
        let label_bounds = tree.bounds(label_tw);
        let shortcut_bounds = tree.bounds(shortcut_tw);

        // Shortcut right edge must reach the trailing edge (minus chevron col).
        let expected_right =
            cut_bounds.x + cut_bounds.width - menu_style.item_padding_horizontal;
        let shortcut_right = shortcut_bounds.x + shortcut_bounds.width;
        assert!(
            (shortcut_right - expected_right).abs() < 1.5,
            "shortcut right edge should be near {} (popup trailing - chevron_col), \
             got {}; Cut item bounds = {:?}, shortcut bounds = {:?}",
            expected_right,
            shortcut_right,
            cut_bounds,
            shortcut_bounds,
        );

        // Gap between label and shortcut — this is the critical check that
        // "no space at all between name and shortcut" bug would fail.
        let label_right = label_bounds.x + label_bounds.width;
        let gap = shortcut_bounds.x - label_right;
        assert!(
            gap > 40.0,
            "expected gap > 40 dp between 'Cut' and 'Ctrl+X', got {} dp \
             (label_right = {}, shortcut_left = {}, cut_bounds = {:?})",
            gap,
            label_right,
            shortcut_bounds.x,
            cut_bounds,
        );
    }

    #[test]
    fn shortcut_has_minimum_gap_even_when_row_is_tight() {
        // When the popup is only slightly wider than the content, the
        // stretch Spacer contributes ~0 dp. The fixed `shortcut_left_gap`
        // (24 dp) must guarantee a visible minimum gap between label and
        // shortcut regardless.
        let mut tree = WidgetTree::new().with_theme(Theme::light_default());
        let item = tree.add(
            MenuItem::new_literal("Cut")
                .on_activate(TestCmd::Cut)
                .shortcut_label("Ctrl+X"),
        );
        // Narrow popup: ~20 dp more than content + shortcut_left_gap.
        // Without the fixed gap, label and shortcut would touch.
        tree.layout(SizeProposal::exact(140.0, 40.0));
        let menu_style = Theme::light_default().components.menu;

        let zstack = tree.child_widget(item, 0);
        let padding = tree.child_widget(zstack, 1);
        let hstack = tree.child_widget(padding, 0);
        let row_children = tree.children(hstack);
        // Expected:
        //   [icon_col, icon_label_gap, label, Spacer,
        //    shortcut_gap, shortcut, chevron_col]
        assert_eq!(
            row_children.len(),
            7,
            "row with shortcut should have 7 children, got {}",
            row_children.len()
        );
        let label_tw = row_children[2];
        let shortcut_tw = row_children[5];
        let label_right = tree.bounds(label_tw).x + tree.bounds(label_tw).width;
        let shortcut_left = tree.bounds(shortcut_tw).x;
        let gap = shortcut_left - label_right;
        assert!(
            gap >= menu_style.shortcut_left_gap - 0.5,
            "gap between label and shortcut should be >= shortcut_left_gap \
             ({} dp), got {} dp",
            menu_style.shortcut_left_gap,
            gap,
        );
    }

    #[test]
    fn menu_item_shortcut_pushes_to_trailing_edge() {
        // In a menu wider than the row's content, the shortcut label must
        // land near the trailing edge AND there must be a large empty
        // gap between the label and the shortcut (that's what the Spacer
        // stretches into).
        //
        // This test walks the tree to find the *inner* TextWidgets rather
        // than using find_by_label, because both the MenuItem and its
        // inner label TextWidget expose the same accessibility name —
        // find_by_label("Cut") would return the MenuItem (300 dp wide),
        // not the inner 20-dp label TextWidget.
        let mut tree = WidgetTree::new().with_theme(Theme::light_default());
        let item = tree.add(
            MenuItem::new_literal("Cut")
                .on_activate(TestCmd::Cut)
                .shortcut_label("Ctrl+X"),
        );
        let popup_width = 300.0_f32;
        tree.layout(SizeProposal::exact(popup_width, 40.0));
        let menu_style = Theme::light_default().components.menu;

        // Walk the tree: MenuItem → ZStack → (RectWidget, Padding → HStack)
        let zstack = tree.child_widget(item, 0);
        // ZStack children: [RectWidget (bg), Padding (with row inside)]
        let padding = tree.child_widget(zstack, 1);
        let hstack = tree.child_widget(padding, 0);
        // HStack children in order:
        //   [icon_col, icon_label_gap, label_TextWidget, Spacer,
        //    shortcut_gap, shortcut_TextWidget, chevron_col]
        let label_tw = tree.child_widget(hstack, 2);
        let shortcut_tw = tree.child_widget(hstack, 5);

        let label_bounds = tree.bounds(label_tw);
        let shortcut_bounds = tree.bounds(shortcut_tw);

        // Shortcut's right edge must reach the trailing edge of the visible
        // row (popup_width minus the chevron column which IS the right
        // padding).
        let expected_right = popup_width - menu_style.item_padding_horizontal;
        let actual_right = shortcut_bounds.x + shortcut_bounds.width;
        assert!(
            (actual_right - expected_right).abs() < 1.5,
            "shortcut right edge should be near {} (popup_width - chevron_col), got {} \
             (shortcut bounds: x={}, w={})",
            expected_right,
            actual_right,
            shortcut_bounds.x,
            shortcut_bounds.width,
        );

        // And there must be a generous gap between the label's right
        // edge and the shortcut's left edge. Without this assertion the
        // "trailing edge" check above can pass even when the label is
        // pushed all the way right to hug the shortcut.
        let label_right = label_bounds.x + label_bounds.width;
        let gap = shortcut_bounds.x - label_right;
        assert!(
            gap > 80.0,
            "Spacer should stretch: expected gap > 80 dp between label and \
             shortcut, got {} dp (label right = {}, shortcut left = {})",
            gap,
            label_right,
            shortcut_bounds.x,
        );
    }

    #[test]
    fn submenu_does_not_open_immediately_on_hover() {
        let mut tree = WidgetTree::new().with_theme(Theme::light_default());
        let item = tree.add(MenuItem::submenu_literal("More", || {
            Box::new(
                crate::menu_list::MenuList::new()
                    .item(MenuItem::new_literal("Sub").on_activate(TestCmd::Cut)),
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
            MenuItem::submenu_literal("More", || {
                Box::new(
                    crate::menu_list::MenuList::new()
                        .item(MenuItem::new_literal("Sub").on_activate(TestCmd::Cut)),
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
        let item = tree.add(MenuItem::submenu_literal("More", || {
            Box::new(
                crate::menu_list::MenuList::new()
                    .item(MenuItem::new_literal("Sub").on_activate(TestCmd::Cut)),
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
        let item = tree.add(MenuItem::submenu_literal("More", || {
            Box::new(
                crate::menu_list::MenuList::new()
                    .item(MenuItem::new_literal("Sub").on_activate(TestCmd::Cut)),
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
                .item(MenuItem::submenu_literal("More", || {
                    Box::new(
                        crate::menu_list::MenuList::new()
                            .item(MenuItem::new_literal("Sub").on_activate(TestCmd::Cut)),
                    )
                }))
                .item(MenuItem::new_literal("Paste").on_activate(TestCmd::Paste)),
        );
        tree.layout(SizeProposal::exact(240.0, 120.0));

        let submenu_item = find_menu_item(&tree, menu, "More");
        let regular_item = find_menu_item(&tree, menu, "Paste");

        tree.pointer_move(tree.bounds(submenu_item).center());
        tree.advance_time(std::time::Duration::from_millis(450));
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
                .item(MenuItem::submenu_literal("More", || {
                    Box::new(
                        crate::menu_list::MenuList::new()
                            .item(MenuItem::new_literal("Sub A").on_activate(TestCmd::Cut)),
                    )
                }))
                .item(MenuItem::submenu_literal("Recent", || {
                    Box::new(
                        crate::menu_list::MenuList::new()
                            .item(MenuItem::new_literal("Sub B").on_activate(TestCmd::Paste)),
                    )
                })),
        );
        tree.layout(SizeProposal::exact(240.0, 120.0));

        let first = find_menu_item(&tree, menu, "More");
        let second = find_menu_item(&tree, menu, "Recent");

        tree.pointer_move(tree.bounds(first).center());
        tree.advance_time(std::time::Duration::from_millis(450));
        assert_eq!(tree.active_overlays().len(), 1);
        assert!(overlay_contains_label(&tree, "Sub A"));

        tree.pointer_move(tree.bounds(second).center());
        assert!(tree.active_overlays().is_empty());

        tree.advance_time(std::time::Duration::from_millis(450));
        assert_eq!(tree.active_overlays().len(), 1);
        assert!(overlay_contains_label(&tree, "Sub B"));
        assert!(!overlay_contains_label(&tree, "Sub A"));
    }

    #[test]
    fn moving_pointer_outside_closes_open_submenu_after_delay() {
        let mut tree = WidgetTree::new().with_theme(Theme::light_default());
        let menu = tree.add(crate::menu_list::MenuList::new().item(MenuItem::submenu_literal(
            "More",
            || {
                Box::new(
                    crate::menu_list::MenuList::new()
                        .item(MenuItem::new_literal("Sub").on_activate(TestCmd::Cut)),
                )
            },
        )));
        tree.layout(SizeProposal::exact(240.0, 80.0));

        let submenu_item = find_menu_item(&tree, menu, "More");
        tree.pointer_move(tree.bounds(submenu_item).center());
        tree.advance_time(std::time::Duration::from_millis(450));
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
            MenuItem::submenu_literal("More", || Box::new(TextWidget::new_literal("placeholder")))
                .submenu_delay(std::time::Duration::from_millis(500)),
        );
        tree.layout(SizeProposal::exact(200.0, 40.0));
        assert!(tree.bounds(item).width > 0.0);
    }

    #[test]
    fn on_activate_fn_fires_closure() {
        let called = Rc::new(Cell::new(false));
        let c = called.clone();
        let mut tree = WidgetTree::new().with_theme(Theme::light_default());
        let item = tree.add(MenuItem::new_literal("Action").on_activate_fn(move |_ctx| {
            c.set(true);
        }));
        tree.layout(SizeProposal::exact(200.0, 40.0));

        tree.click(item);
        assert!(called.get());
    }

    #[test]
    fn on_activate_fn_disabled_ignores() {
        let called = Rc::new(Cell::new(false));
        let c = called.clone();
        let mut tree = WidgetTree::new().with_theme(Theme::light_default());
        let item = tree.add(
            MenuItem::new_literal("Nope")
                .on_activate_fn(move |_ctx| c.set(true))
                .enabled(false),
        );
        tree.layout(SizeProposal::exact(200.0, 40.0));

        tree.click(item);
        assert!(!called.get());
    }
}
