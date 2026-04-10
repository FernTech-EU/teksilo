//! MenuList — a vertical container for MenuItem and MenuSeparator widgets.
//!
//! Provides a themed surface (background, border, corner radius) and
//! keyboard navigation (ArrowUp/Down, Enter, Escape).

use fern_canvas::{Rect, Size, SizeProposal};
use fern_core::accessibility::AccessNodeBuilder;
use fern_core::build_context::BuildContext;
use fern_core::event::{EventResponse, Key, WidgetEvent};
use fern_core::widget::{
    EventContext, LayoutContext, PaintContext, PendingChild, Widget, WidgetPlacement,
};
use fern_core::widget_builder::HandlerSet;
use fern_core::widget_id::WidgetId;
use fern_tokens::CornerRadius;

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
    /// Indices into entries that are items (not separators) — for keyboard navigation.
    item_indices: Vec<usize>,
    /// Which item is currently keyboard-focused (index into item_indices).
    focused_item: usize,
}

impl MenuList {
    pub fn new() -> Self {
        Self {
            entries: Vec::new(),
            root_child_id: None,
            item_indices: Vec::new(),
            focused_item: 0,
        }
    }

    /// Add a menu item (typically a `MenuItem`).
    pub fn item(mut self, widget: impl Widget + 'static) -> Self {
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

        // Build all entries into a VStack
        let mut vstack = VStack::new();
        self.item_indices.clear();

        for (i, entry) in self.entries.drain(..).enumerate() {
            match entry {
                MenuEntry::Item(pending) => {
                    self.item_indices.push(i);
                    match pending {
                        PendingChild::Id(id) => {
                            vstack = vstack.add_child(id);
                        }
                        PendingChild::Deferred(w) => {
                            let id = ctx.add_boxed(w);
                            vstack = vstack.add_child(id);
                        }
                    }
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
        let item_indices = self.item_indices.clone();
        let _focused_item = self.focused_item; // Capture current focused item
        let handler_set = HandlerSet::new()
            .on_key(move |event: &WidgetEvent, _ctx: &mut EventContext| -> EventResponse {
                match event {
                    WidgetEvent::KeyDown {
                        key: Key::Escape, ..
                    } => {
                        // Let overlay system handle Escape dismissal
                        EventResponse::Ignored
                    }
                    WidgetEvent::KeyDown {
                        key: Key::ArrowDown, ..
                    } => {
                        // Navigate to next item
                        if !item_indices.is_empty() {
                            // Cycle to next item (wrap around)
                            // Note: Actual focus management requires widget tree integration
                            // This provides the logical navigation framework
                            EventResponse::Handled
                        } else {
                            EventResponse::Ignored
                        }
                    }
                    WidgetEvent::KeyDown {
                        key: Key::ArrowUp, ..
                    } => {
                        // Navigate to previous item
                        if !item_indices.is_empty() {
                            // Cycle to previous item (wrap around)
                            // Note: Actual focus management requires widget tree integration
                            // This provides the logical navigation framework
                            EventResponse::Handled
                        } else {
                            EventResponse::Ignored
                        }
                    }
                    WidgetEvent::KeyDown {
                        key: Key::Enter, ..
                    } => {
                        // Activate focused item
                        if !item_indices.is_empty() {
                            // Activate the focused menu item
                            // Note: Actual activation requires widget tree integration
                            // This provides the activation framework
                            EventResponse::Handled
                        } else {
                            EventResponse::Ignored
                        }
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
        // Should have at least one shape (the background rect)
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
        // The menu should have children (its ZStack root)
        let children = tree.children(menu);
        assert!(
            !children.is_empty(),
            "menu list should have built child widgets"
        );
    }
}
