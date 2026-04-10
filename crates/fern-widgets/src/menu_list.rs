//! MenuList — a vertical container for MenuItem and MenuSeparator widgets.
//!
//! Provides a themed surface (background, border, corner radius) and
//! keyboard navigation (ArrowUp/Down, Enter, Escape).

use fern_canvas::{Rect, Size, SizeProposal};
use fern_core::accessibility::AccessNodeBuilder;
use fern_core::build_context::BuildContext;
use fern_core::event::{EventResponse, Key, WidgetEvent};
use fern_core::signal::Signal;
use fern_core::widget::{
    EventContext, LayoutContext, PaintContext, PendingChild, Widget, WidgetPlacement,
};
use fern_core::widget_builder::HandlerSet;
use fern_core::widget_id::WidgetId;
use fern_tokens::{Color, CornerRadius};

use crate::primitives::{Padding, RectWidget, VStack, ZStack};

/// Marker for whether a pending item is a menu item or a separator.
enum MenuEntry {
    Item(PendingChild),
    Separator,
}

/// A separator line within a MenuList.
#[derive(Debug)]
pub struct MenuSeparator;

impl Widget for MenuSeparator {
    fn size_that_fits(&self, proposal: SizeProposal, _ctx: &LayoutContext) -> Size {
        let width = proposal.width.unwrap_or(0.0);
        Size::new(width, 9.0) // 1px line + 4px padding top + 4px padding bottom
    }

    fn paint(&self, bounds: Rect, canvas: &mut fern_canvas::Canvas, ctx: &PaintContext) {
        let color = ctx.theme.colors.border.with_alpha(0.3);
        let y = bounds.y + bounds.height / 2.0;
        canvas.fill_rect(
            Rect::new(bounds.x + 8.0, y, bounds.width - 16.0, 1.0),
            color,
        );
    }

    fn accessibility(&self, builder: &mut AccessNodeBuilder) {
        builder.set_role(fern_core::accesskit::Role::Splitter);
    }
}

// TODO(milestone-7): Add `max_visible_items` option to MenuList. When item count exceeds
// the limit, show a scrollable list with arrow headers/footers. Blocked on ListView.

/// Wrapper that adds a keyboard-focus highlight behind a menu item.
/// The highlight is driven by a shared `focused_index` signal — when
/// `focused_index == Some(my_index)`, a subtle background appears.
/// The binding registry automatically marks this widget for repaint
/// when the signal changes (same mechanism as ComboBox DropdownItem).
#[derive(Debug)]
struct KeyboardHighlightWrapper {
    item_id: WidgetId,
    index: usize,
    focused_index: Signal<Option<usize>>,
    root_child_id: Option<WidgetId>,
}

impl Widget for KeyboardHighlightWrapper {
    fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
        let theme = ctx.theme().clone();
        let index = self.index;

        // Background highlight driven by focused_index signal.
        // Registered via bind_background → Prop::Bound → binding registry.
        let bg_color = self.focused_index.map({
            let primary = theme.colors.primary;
            move |focused| {
                if *focused == Some(index) {
                    primary.with_alpha(0.10)
                } else {
                    Color::TRANSPARENT
                }
            }
        });

        let bg = RectWidget::new().bind_background(bg_color);
        let bg_id = ctx.add(bg);

        let zstack = ZStack::new()
            .add_child(bg_id)
            .add_child(self.item_id);
        let root_id = ctx.add(zstack);
        self.root_child_id = Some(root_id);

        vec![root_id]
    }

    fn size_that_fits(&self, proposal: SizeProposal, ctx: &LayoutContext) -> Size {
        self.root_child_id
            .and_then(|id| ctx.child_size(id, proposal))
            .unwrap_or_else(|| proposal.resolve(0.0, 32.0))
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

    fn children(&self) -> Vec<WidgetId> {
        self.root_child_id.into_iter().collect()
    }
}

/// A themed vertical menu container.
///
/// ```ignore
/// MenuList::new()
///     .item(MenuItem::new("Cut").on_activate(AppCmd::Cut))
///     .separator()
///     .item(MenuItem::new("Paste").on_activate(AppCmd::Paste))
/// ```
pub struct MenuList {
    entries: Vec<MenuEntry>,
    root_child_id: Option<WidgetId>,
    /// Widget IDs of actual menu items (not separators), for keyboard navigation.
    item_widget_ids: Vec<WidgetId>,
    /// Whether each item (by index into item_widget_ids) is a submenu trigger.
    submenu_flags: Vec<bool>,
}

impl MenuList {
    pub fn new() -> Self {
        Self {
            entries: Vec::new(),
            root_child_id: None,
            item_widget_ids: Vec::new(),
            submenu_flags: Vec::new(),
        }
    }

    /// Add a menu item (typically a `MenuItem`).
    pub fn item(mut self, widget: impl Widget + 'static) -> Self {
        // Detect submenu items via Any downcast before boxing
        let is_submenu = (&widget as &dyn std::any::Any)
            .downcast_ref::<crate::menu_item::MenuItem>()
            .is_some_and(|mi| mi.is_submenu());
        self.submenu_flags.push(is_submenu);
        self.entries
            .push(MenuEntry::Item(PendingChild::Deferred(Box::new(widget))));
        self
    }

    /// Add a separator line.
    pub fn separator(mut self) -> Self {
        self.entries.push(MenuEntry::Separator);
        self
    }
}

impl Default for MenuList {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Debug for MenuList {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MenuList")
            .field("entries", &self.entries.len())
            .finish()
    }
}

impl Widget for MenuList {
    fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
        let theme = ctx.theme().clone();

        // Keyboard-focused item index (shared with the key handler and wrappers).
        // The binding registry propagates repaints when this changes.
        let focused_index: Signal<Option<usize>> = ctx.signal(None);

        // Build all entries into a VStack, wrapping items in highlight wrappers
        let mut vstack = VStack::new();
        self.item_widget_ids.clear();
        let mut item_counter = 0_usize;

        for entry in self.entries.drain(..) {
            match entry {
                MenuEntry::Item(pending) => {
                    let item_id = match pending {
                        PendingChild::Id(id) => id,
                        PendingChild::Deferred(w) => ctx.add_boxed(w),
                    };
                    self.item_widget_ids.push(item_id);

                    // Wrap in a highlight container driven by focused_index
                    let wrapper = KeyboardHighlightWrapper {
                        item_id,
                        index: item_counter,
                        focused_index: focused_index.clone(),
                        root_child_id: None,
                    };
                    vstack = vstack.child(wrapper);
                    item_counter += 1;
                }
                MenuEntry::Separator => {
                    vstack = vstack.child(MenuSeparator);
                }
            }
        }

        let vstack_id = ctx.add(vstack);

        let padding = Padding::uniform(4.0).set_child(vstack_id);
        let padding_id = ctx.add(padding);

        // Themed surface background
        let bg = RectWidget::new()
            .background(theme.colors.surface)
            .border_color(theme.colors.border.with_alpha(0.3))
            .border_width(1.0)
            .corner_radius(CornerRadius::uniform(theme.shape.radius_sm));
        let bg_id = ctx.add(bg);

        let zstack = ZStack::new().add_child(bg_id).add_child(padding_id);
        let root_id = ctx.add(zstack);

        self.root_child_id = Some(root_id);

        // Keyboard navigation handler
        let item_count = self.item_widget_ids.len();
        let item_ids = self.item_widget_ids.clone();
        let sub_flags = self.submenu_flags.clone();
        let handler_set = HandlerSet::new()
            .on_key(move |event: &WidgetEvent, ctx: &mut EventContext| -> EventResponse {
                match event {
                    WidgetEvent::KeyDown {
                        key: Key::ArrowDown,
                        ..
                    } => {
                        if item_count == 0 {
                            return EventResponse::Ignored;
                        }
                        let current = focused_index.get().unwrap_or(usize::MAX);
                        let next = if current >= item_count - 1 {
                            0
                        } else {
                            current + 1
                        };
                        focused_index.set(Some(next));
                        EventResponse::Handled
                    }
                    WidgetEvent::KeyDown {
                        key: Key::ArrowUp, ..
                    } => {
                        if item_count == 0 {
                            return EventResponse::Ignored;
                        }
                        let current = focused_index.get().unwrap_or(0);
                        let next = if current == 0 {
                            item_count - 1
                        } else {
                            current - 1
                        };
                        focused_index.set(Some(next));
                        EventResponse::Handled
                    }
                    WidgetEvent::KeyDown {
                        key: Key::Enter | Key::Space,
                        ..
                    } => {
                        // Activate the focused item via synthetic click.
                        if let Some(idx) = focused_index.get() {
                            if idx < item_ids.len() {
                                ctx.synthetic_click(item_ids[idx]);
                                return EventResponse::Handled;
                            }
                        }
                        EventResponse::Ignored
                    }
                    WidgetEvent::KeyDown {
                        key: Key::ArrowRight,
                        ..
                    } => {
                        // Only open submenus; for non-submenu items, let it bubble
                        // to MenuOverlayHost which navigates to the next bar menu.
                        if let Some(idx) = focused_index.get() {
                            if idx < sub_flags.len() && sub_flags[idx] {
                                ctx.synthetic_click(item_ids[idx]);
                                return EventResponse::Handled;
                            }
                        }
                        EventResponse::Ignored
                    }
                    WidgetEvent::KeyDown {
                        key: Key::ArrowLeft | Key::Escape,
                        ..
                    } => {
                        // Let it bubble to MenuOverlayHost (for bar navigation)
                        // or the tree-level Escape handler (for overlay dismissal).
                        EventResponse::Ignored
                    }
                    _ => EventResponse::Ignored,
                }
            })
            .focusable(true);

        ctx.apply_self_handlers(handler_set);

        vec![root_id]
    }

    fn size_that_fits(&self, proposal: SizeProposal, ctx: &LayoutContext) -> Size {
        match self.root_child_id {
            Some(id) => {
                // Menu lists size to their content, with a minimum width
                let child_size = ctx
                    .child_size(id, proposal)
                    .unwrap_or_else(|| proposal.resolve(0.0, 0.0));
                Size::new(child_size.width.max(120.0), child_size.height)
            }
            None => proposal.resolve(120.0, 0.0),
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
        builder.set_role(fern_core::accesskit::Role::Menu);
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
    use crate::menu_item::MenuItem;
    use fern_core::app_command::AppCommand;
    use fern_core::widget_tree::WidgetTree;
    use fern_tokens::Theme;
    use std::cell::RefCell;
    use std::rc::Rc;

    #[derive(Debug, Clone, PartialEq)]
    enum TestCmd {
        Cut,
        Copy,
        Paste,
    }
    impl AppCommand for TestCmd {}

    fn setup_menu() -> (WidgetTree, WidgetId) {
        let mut tree = WidgetTree::new().with_theme(Theme::light_default());
        let menu = tree.add(
            MenuList::new()
                .item(MenuItem::new("Cut").on_activate(TestCmd::Cut))
                .separator()
                .item(MenuItem::new("Copy").on_activate(TestCmd::Copy))
                .item(MenuItem::new("Paste").on_activate(TestCmd::Paste)),
        );
        tree.layout(SizeProposal::exact(300.0, 300.0));
        (tree, menu)
    }

    fn capture_commands(tree: &mut WidgetTree) -> Rc<RefCell<Vec<TestCmd>>> {
        let commands = Rc::new(RefCell::new(Vec::new()));
        let captured = commands.clone();
        tree.on_command(move |cmd: &TestCmd| {
            captured.borrow_mut().push(cmd.clone());
        });
        commands
    }

    #[test]
    fn menu_list_builds_and_lays_out() {
        let (tree, menu) = setup_menu();
        let bounds = tree.bounds(menu);
        assert!(bounds.width >= 120.0, "menu should have minimum width");
        assert!(bounds.height > 0.0, "menu should have content height");
    }

    #[test]
    fn menu_list_has_surface_background() {
        let (mut tree, _) = setup_menu();
        let frame = tree.render();
        assert!(!frame.shapes.is_empty());
    }

    #[test]
    fn accessibility_role() {
        let (tree, menu) = setup_menu();
        let info = tree.accessibility_node(menu);
        assert_eq!(info.role(), fern_core::accesskit::Role::Menu);
    }

    #[test]
    fn separator_accessibility_role() {
        let mut tree = WidgetTree::new().with_theme(Theme::light_default());
        let sep = tree.add(MenuSeparator);
        tree.layout(SizeProposal::exact(200.0, 20.0));
        let info = tree.accessibility_node(sep);
        assert_eq!(info.role(), fern_core::accesskit::Role::Splitter);
    }

    #[test]
    fn menu_list_contains_items_and_separators() {
        let (tree, menu) = setup_menu();
        let children = tree.children(menu);
        assert!(
            !children.is_empty(),
            "menu list should have built child widgets"
        );
    }

    #[test]
    fn arrow_down_selects_first_item() {
        let (mut tree, menu) = setup_menu();
        let commands = capture_commands(&mut tree);
        tree.focus(menu);

        tree.press_key(Key::ArrowDown, fern_core::event::Modifiers::NONE);
        tree.press_key(Key::Enter, fern_core::event::Modifiers::NONE);
        tree.layout(SizeProposal::exact(300.0, 300.0));

        assert_eq!(&*commands.borrow(), &[TestCmd::Cut]);
    }

    #[test]
    fn arrow_up_wraps_to_last_item() {
        let (mut tree, menu) = setup_menu();
        let commands = capture_commands(&mut tree);
        tree.focus(menu);

        // ArrowUp from initial state wraps to last item
        tree.press_key(Key::ArrowUp, fern_core::event::Modifiers::NONE);
        tree.press_key(Key::Enter, fern_core::event::Modifiers::NONE);
        tree.layout(SizeProposal::exact(300.0, 300.0));

        assert_eq!(&*commands.borrow(), &[TestCmd::Paste]);
    }
}
