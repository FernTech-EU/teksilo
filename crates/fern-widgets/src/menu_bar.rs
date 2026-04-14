//! MenuBar — a horizontal menu bar with dropdown menus.
//!
//! # FernUI
//! ```ignore
//! MenuBar::new()
//!     .menu_literal("File", || Box::new(
//!         MenuList::new()
//!             .item(MenuItem::new_literal("New").on_activate(Cmd::New))
//!             .separator()
//!             .item(MenuItem::new_literal("Quit").on_activate(Cmd::Quit))
//!     ))
//!     .menu_literal("Edit", || Box::new(
//!         MenuList::new()
//!             .item(MenuItem::new_literal("Cut").on_activate(Cmd::Cut))
//!             .item(MenuItem::new_literal("Copy").on_activate(Cmd::Copy))
//!     ))
//!     .trailing_slot(Button::new_literal("Settings").on_activate(Cmd::Settings))
//! ```

use fern_canvas::{Rect, Size, SizeProposal};
use fern_core::accessibility::AccessNodeBuilder;
use fern_core::build_context::BuildContext;
use fern_core::event::{EventResponse, Key, WidgetEvent};
use fern_core::signal::Signal;
use fern_core::widget::{
    CursorIcon, EventContext, LayoutContext, PendingChild, Widget, WidgetPlacement,
};
use fern_core::widget_builder::HandlerSet;
use fern_core::widget_id::WidgetId;
use fern_tokens::Color;

use crate::menu_context::MenuContext;
use crate::primitives::{HStack, Padding, RectWidget, Spacer, TextWidget, ZStack};

// ---------------------------------------------------------------------------
// MenuBarEntry — pending menu definition
// ---------------------------------------------------------------------------

struct MenuBarEntry {
    label: String,
    factory: Box<dyn Fn() -> Box<dyn Widget>>,
}

// ---------------------------------------------------------------------------
// MenuBar — public widget
// ---------------------------------------------------------------------------

/// A horizontal menu bar with dropdown menus.
///
/// Supports the Slot system (architecture Section 5.3):
/// - `leading_slot`: content before the menu buttons (e.g., app icon)
/// - `trailing_slot`: content after the menu buttons (e.g., search, user avatar)
pub struct MenuBar {
    entries: Vec<MenuBarEntry>,
    leading_slot: Vec<PendingChild>,
    trailing_slot: Vec<PendingChild>,
    root_child_id: Option<WidgetId>,
}

impl MenuBar {
    pub fn new() -> Self {
        Self {
            entries: Vec::new(),
            leading_slot: Vec::new(),
            trailing_slot: Vec::new(),
            root_child_id: None,
        }
    }

    pub fn menu(
        mut self,
        label: impl Into<fern_i18n::LocalizedString>,
        factory: impl Fn() -> Box<dyn Widget> + 'static,
    ) -> Self {
        let ls: fern_i18n::LocalizedString = label.into();
        self.entries.push(MenuBarEntry {
            label: ls.resolve_now(),
            factory: Box::new(factory),
        });
        self
    }

    /// Shim (permanent, `#[doc(hidden)]`) for `menu(...)` accepting a raw label.
    #[doc(hidden)]
    pub fn menu_literal(
        self,
        label: impl Into<String>,
        factory: impl Fn() -> Box<dyn Widget> + 'static,
    ) -> Self {
        self.menu(fern_i18n::LocalizedString::literal(label), factory)
    }

    pub fn leading_slot(mut self, widget: impl Widget + 'static) -> Self {
        self.leading_slot
            .push(PendingChild::Deferred(Box::new(widget)));
        self
    }

    pub fn trailing_slot(mut self, widget: impl Widget + 'static) -> Self {
        self.trailing_slot
            .push(PendingChild::Deferred(Box::new(widget)));
        self
    }
}

impl Default for MenuBar {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Debug for MenuBar {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MenuBar")
            .field("entries", &self.entries.len())
            .finish()
    }
}

// ---------------------------------------------------------------------------
// MenuBarTrigger — internal trigger label
// ---------------------------------------------------------------------------

#[derive(Debug)]
struct MenuBarTrigger {
    label: String,
    index: usize,
    menu_ctx: MenuContext,
    root_child_id: Option<WidgetId>,
}

impl Widget for MenuBarTrigger {
    fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
        let theme = ctx.theme().clone();
        let index = self.index;
        let menu_ctx = self.menu_ctx.clone();

        // Background and text color derived from the shared open_index signal.
        // Single signal source → binding registry handles repaints automatically.
        let bg_color = menu_ctx.open_index.map({
            let primary = theme.colors.accent;
            move |open| {
                if *open == Some(index) {
                    primary.with_alpha(0.12)
                } else {
                    Color::TRANSPARENT
                }
            }
        });

        let text_color = menu_ctx.open_index.map({
            let on_surface = theme.colors.text_primary;
            move |open| {
                if *open == Some(index) {
                    on_surface
                } else {
                    on_surface.with_alpha(0.8)
                }
            }
        });

        let label = TextWidget::new_literal(&self.label)
            .style(theme.typography.small.clone())
            .bind_color(text_color);
        let label_id = ctx.add(label);

        let menu_style = theme.components.menu;
        let padding = Padding::symmetric(
            4.0,
            menu_style.item_padding_horizontal,
        )
        .set_child(label_id);
        let padding_id = ctx.add(padding);

        let bg = RectWidget::new()
            .bind_background(bg_color)
            .corner_radius(fern_tokens::CornerRadius::uniform(theme.shape.radius_control));
        let bg_id = ctx.add(bg);

        let zstack = ZStack::new().add_child(bg_id).add_child(padding_id);
        let root_id = ctx.add(zstack);
        self.root_child_id = Some(root_id);

        let handler_set = HandlerSet::new()
            .on_tap({
                let menu_ctx = menu_ctx.clone();
                move |ctx: &mut EventContext| {
                    if menu_ctx.open_index.get() == Some(index) {
                        menu_ctx.close(ctx);
                    } else {
                        menu_ctx.open_at(index, ctx);
                    }
                }
            })
            .on_hover({
                let menu_ctx = menu_ctx.clone();
                move |entered: bool, ctx: &mut EventContext| {
                    if entered {
                        // If another menu is open, switch immediately (no delay)
                        let current = menu_ctx.open_index.get();
                        if current.is_some() && current != Some(index) {
                            menu_ctx.open_at(index, ctx);
                        }
                    }
                }
            })
            .on_key({
                let menu_ctx = menu_ctx.clone();
                move |event: &WidgetEvent, ctx: &mut EventContext| -> EventResponse {
                    match event {
                        WidgetEvent::KeyDown {
                            key: Key::ArrowDown | Key::Enter | Key::Space,
                            ..
                        } => {
                            menu_ctx.open_at(index, ctx);
                            EventResponse::Handled
                        }
                        WidgetEvent::KeyDown {
                            key: Key::ArrowLeft,
                            ..
                        } => {
                            menu_ctx.navigate(-1, ctx);
                            EventResponse::Handled
                        }
                        WidgetEvent::KeyDown {
                            key: Key::ArrowRight,
                            ..
                        } => {
                            menu_ctx.navigate(1, ctx);
                            EventResponse::Handled
                        }
                        _ => EventResponse::Ignored,
                    }
                }
            })
            .focusable(true)
            .cursor(CursorIcon::Pointer);

        ctx.apply_self_handlers(handler_set);

        vec![root_id]
    }

    fn size_that_fits(&self, proposal: SizeProposal, ctx: &LayoutContext) -> Size {
        match self.root_child_id {
            Some(id) => ctx
                .child_size(id, proposal)
                .unwrap_or_else(|| proposal.resolve(0.0, 28.0)),
            None => proposal.resolve(60.0, 28.0),
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
    }

    fn children(&self) -> Vec<WidgetId> {
        self.root_child_id.into_iter().collect()
    }
}

// ---------------------------------------------------------------------------
// MenuOverlayHost — wraps dropdown content, handles focus + cross-menu keys
// ---------------------------------------------------------------------------

/// Wraps dropdown menu content (typically a MenuList). Responsibilities:
/// - Resets `open_index` when focus is lost (overlay dismissed)
/// - Handles ArrowLeft/Right for cross-menu navigation (bubbles up from MenuList)
#[derive(Debug)]
struct MenuOverlayHost {
    inner: Option<Box<dyn Widget>>,
    menu_ctx: MenuContext,
    menu_index: usize,
    inner_id: Option<WidgetId>,
}

impl Widget for MenuOverlayHost {
    fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
        let inner_widget = self.inner.take().expect("MenuOverlayHost built twice");
        let id = ctx.add_boxed(inner_widget);
        self.inner_id = Some(id);

        // Register inner widget as the focus target for this menu index
        self.menu_ctx.set_focus_id(self.menu_index, id);

        let menu_ctx = self.menu_ctx.clone();
        let menu_index = self.menu_index;
        let handler_set = HandlerSet::new()
            .on_focus({
                let menu_ctx = menu_ctx.clone();
                move |gained: bool, ctx: &mut EventContext| {
                    if !gained && menu_ctx.open_index.get() == Some(menu_index) {
                        // Overlay was dismissed — close the menu and restore focus
                        menu_ctx.close(ctx);
                    }
                }
            })
            .on_key({
                let menu_ctx = menu_ctx.clone();
                move |event: &WidgetEvent, ctx: &mut EventContext| -> EventResponse {
                    // These keys bubble up from the inner MenuList when it returns Ignored
                    match event {
                        WidgetEvent::KeyDown {
                            key: Key::ArrowLeft,
                            ..
                        } => {
                            menu_ctx.navigate(-1, ctx);
                            EventResponse::Handled
                        }
                        WidgetEvent::KeyDown {
                            key: Key::ArrowRight,
                            ..
                        } => {
                            menu_ctx.navigate(1, ctx);
                            EventResponse::Handled
                        }
                        WidgetEvent::KeyDown {
                            key: Key::Escape, ..
                        } => {
                            menu_ctx.close(ctx);
                            EventResponse::Handled
                        }
                        _ => EventResponse::Ignored,
                    }
                }
            });
        // NOT focusable — the inner MenuList receives focus directly.
        // ArrowLeft/Right and FocusLost bubble from MenuList through here.
        ctx.apply_self_handlers(handler_set);

        vec![id]
    }

    fn size_that_fits(&self, proposal: SizeProposal, ctx: &LayoutContext) -> Size {
        self.inner_id
            .and_then(|id| ctx.child_size(id, proposal))
            .unwrap_or_else(|| proposal.resolve(0.0, 0.0))
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
        self.inner_id.into_iter().collect()
    }
}

// ---------------------------------------------------------------------------
// MenuBar Widget impl
// ---------------------------------------------------------------------------

impl Widget for MenuBar {
    fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
        let theme = ctx.theme().clone();

        let open_index: Signal<Option<usize>> = ctx.signal(None);
        let menu_ctx = MenuContext::new(open_index);

        // Build the full row: [leading_slot | triggers... | Spacer | trailing_slot]
        let mut row = HStack::new().spacing(2.0);

        // Leading slot
        for pending in self.leading_slot.drain(..) {
            match pending {
                PendingChild::Id(id) => row = row.add_child(id),
                PendingChild::Deferred(w) => {
                    let id = ctx.add_boxed(w);
                    row = row.add_child(id);
                }
            }
        }

        // Menu triggers + content
        let mut trigger_ids = Vec::new();
        let mut content_ids = Vec::new();

        for (i, entry) in self.entries.drain(..).enumerate() {
            // Wrap factory output in MenuOverlayHost for focus/key handling
            let host = MenuOverlayHost {
                inner: Some((entry.factory)()),
                menu_ctx: menu_ctx.clone(),
                menu_index: i,
                inner_id: None,
            };
            let content_id = ctx.add(host);
            ctx.set_dormant(content_id);

            let trigger = MenuBarTrigger {
                label: entry.label,
                index: i,
                menu_ctx: menu_ctx.clone(),
                root_child_id: None,
            };
            let trigger_id = ctx.add(trigger);
            row = row.add_child(trigger_id);

            trigger_ids.push(trigger_id);
            content_ids.push(content_id);
        }

        // Register all trigger/content IDs in the context.
        // focus_id is initially content_id; MenuOverlayHost::build() will
        // overwrite it with the actual inner MenuList ID.
        for (i, (&tid, &cid)) in trigger_ids.iter().zip(content_ids.iter()).enumerate() {
            menu_ctx.register(i, tid, cid, cid);
        }

        // Spacer pushes triggers left, trailing slot right
        row = row.child(Spacer::new());

        // Trailing slot
        for pending in self.trailing_slot.drain(..) {
            match pending {
                PendingChild::Id(id) => row = row.add_child(id),
                PendingChild::Deferred(w) => {
                    let id = ctx.add_boxed(w);
                    row = row.add_child(id);
                }
            }
        }

        let row_id = ctx.add(row);

        let bg = RectWidget::new()
            .background(theme.colors.surface_main)
            .border_color(theme.colors.border.with_alpha(0.2))
            .border_width(0.0);
        let bg_id = ctx.add(bg);

        let padding = Padding::symmetric(0.0, 2.0).set_child(row_id);
        let padding_id = ctx.add(padding);

        let zstack = ZStack::new().add_child(bg_id).add_child(padding_id);
        let root_id = ctx.add(zstack);
        self.root_child_id = Some(root_id);

        vec![root_id]
    }

    fn size_that_fits(&self, proposal: SizeProposal, ctx: &LayoutContext) -> Size {
        match self.root_child_id {
            Some(id) => {
                let content_proposal = SizeProposal {
                    width: proposal.width,
                    height: None,
                };
                let size = ctx
                    .child_size(id, content_proposal)
                    .unwrap_or_else(|| proposal.resolve(0.0, 0.0));
                Size::new(proposal.width.unwrap_or(size.width), size.height)
            }
            None => proposal.resolve(0.0, 0.0),
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
        builder.set_role(fern_core::accesskit::Role::MenuBar);
    }

    fn children(&self) -> Vec<WidgetId> {
        self.root_child_id.into_iter().collect()
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::menu_item::MenuItem;
    use crate::menu_list::MenuList;
    use crate::primitives::{Expand, VStack};
    use fern_core::accesskit::Role;
    use fern_core::app_command::AppCommand;
    use fern_core::widget_tree::WidgetTree;
    use fern_tokens::Theme;
    use std::cell::RefCell;
    use std::rc::Rc;

    #[derive(Debug, Clone, PartialEq)]
    enum TestCmd {
        New,
        Open,
        Cut,
        Copy,
    }
    impl AppCommand for TestCmd {}

    fn setup_menu_bar() -> (WidgetTree, WidgetId) {
        let mut tree = WidgetTree::new().with_theme(Theme::light_default());
        let bar = tree.add(
            MenuBar::new()
                .menu_literal("File", || {
                    Box::new(
                        MenuList::new()
                            .item(MenuItem::new_literal("New").on_activate(TestCmd::New))
                            .item(MenuItem::new_literal("Open").on_activate(TestCmd::Open)),
                    )
                })
                .menu_literal("Edit", || {
                    Box::new(
                        MenuList::new()
                            .item(MenuItem::new_literal("Cut").on_activate(TestCmd::Cut))
                            .item(MenuItem::new_literal("Copy").on_activate(TestCmd::Copy)),
                    )
                })
                .menu_literal("View", || {
                    Box::new(
                        MenuList::new()
                            .item(MenuItem::new_literal("Zoom In").on_activate(TestCmd::New))
                            .item(MenuItem::new_literal("Zoom Out").on_activate(TestCmd::Open)),
                    )
                }),
        );
        tree.layout(SizeProposal::exact(600.0, 400.0));
        (tree, bar)
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

    fn find_menu_item_by_name(tree: &WidgetTree, root: WidgetId, name: &str) -> WidgetId {
        descendants(tree, root)
            .into_iter()
            .find(|&id| {
                let info = tree.accessibility_node(id);
                info.role() == Role::MenuItem && info.name() == Some(name)
            })
            .unwrap_or_else(|| panic!("menu item '{name}' not found"))
    }

    fn trigger_center(tree: &WidgetTree, root: WidgetId, label: &str) -> fern_canvas::Point {
        let trigger = find_menu_item_by_name(tree, root, label);
        tree.bounds(trigger).center()
    }

    fn overlay_labels(tree: &WidgetTree) -> Vec<String> {
        let mut labels = Vec::new();
        for root in tree.overlay_manager().active_content_ids() {
            for id in descendants(tree, root) {
                let info = tree.accessibility_node(id);
                if let Some(name) = info.name() {
                    labels.push(name.to_string());
                }
            }
        }
        labels.sort();
        labels.dedup();
        labels
    }

    fn overlay_contains_label(tree: &WidgetTree, label: &str) -> bool {
        overlay_labels(tree).iter().any(|item| item == label)
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
    fn builds_and_lays_out() {
        let (tree, bar) = setup_menu_bar();
        let bounds = tree.bounds(bar);
        assert!(bounds.width > 0.0);
        assert!(bounds.height > 0.0);
    }

    #[test]
    fn accessibility_role() {
        let (tree, bar) = setup_menu_bar();
        let info = tree.accessibility_node(bar);
        assert_eq!(info.role(), fern_core::accesskit::Role::MenuBar);
    }

    #[test]
    fn click_opens_dropdown() {
        let (mut tree, bar) = setup_menu_bar();
        let file_center = trigger_center(&tree, bar, "File");
        assert!(tree.active_overlays().is_empty());

        tree.dispatch_event(WidgetEvent::PointerDown {
            position: file_center,
            button: fern_core::event::PointerButton::Primary,
            modifiers: fern_core::event::Modifiers::NONE,
        });
        tree.dispatch_event(WidgetEvent::PointerUp {
            position: file_center,
            button: fern_core::event::PointerButton::Primary,
            modifiers: fern_core::event::Modifiers::NONE,
        });
        tree.layout(SizeProposal::exact(600.0, 400.0));

        assert_eq!(tree.active_overlays().len(), 1);
        assert!(overlay_contains_label(&tree, "New"));
        assert!(overlay_contains_label(&tree, "Open"));
        assert!(!overlay_contains_label(&tree, "Cut"));
    }

    #[test]
    fn click_toggle_closes() {
        let (mut tree, bar) = setup_menu_bar();
        let file_center = trigger_center(&tree, bar, "File");

        // Open
        tree.dispatch_event(WidgetEvent::PointerDown {
            position: file_center,
            button: fern_core::event::PointerButton::Primary,
            modifiers: fern_core::event::Modifiers::NONE,
        });
        tree.dispatch_event(WidgetEvent::PointerUp {
            position: file_center,
            button: fern_core::event::PointerButton::Primary,
            modifiers: fern_core::event::Modifiers::NONE,
        });
        tree.layout(SizeProposal::exact(600.0, 400.0));
        assert_eq!(tree.active_overlays().len(), 1);

        // Close (second click on same trigger)
        tree.dispatch_event(WidgetEvent::PointerDown {
            position: file_center,
            button: fern_core::event::PointerButton::Primary,
            modifiers: fern_core::event::Modifiers::NONE,
        });
        tree.dispatch_event(WidgetEvent::PointerUp {
            position: file_center,
            button: fern_core::event::PointerButton::Primary,
            modifiers: fern_core::event::Modifiers::NONE,
        });

        assert!(
            tree.active_overlays().is_empty(),
            "clicking the same trigger again should close the menu (got {})",
            tree.active_overlays().len()
        );
    }

    #[test]
    fn hover_switches_open_menu() {
        let (mut tree, bar) = setup_menu_bar();
        let file_center = trigger_center(&tree, bar, "File");
        let edit_center = trigger_center(&tree, bar, "Edit");

        // Open File
        tree.dispatch_event(WidgetEvent::PointerDown {
            position: file_center,
            button: fern_core::event::PointerButton::Primary,
            modifiers: fern_core::event::Modifiers::NONE,
        });
        tree.dispatch_event(WidgetEvent::PointerUp {
            position: file_center,
            button: fern_core::event::PointerButton::Primary,
            modifiers: fern_core::event::Modifiers::NONE,
        });
        tree.layout(SizeProposal::exact(600.0, 400.0));
        assert_eq!(tree.active_overlays().len(), 1);
        assert!(overlay_contains_label(&tree, "New"));

        // Hover Edit — should switch
        tree.pointer_move(edit_center);
        tree.layout(SizeProposal::exact(600.0, 400.0));
        assert_eq!(tree.active_overlays().len(), 1);
        assert!(overlay_contains_label(&tree, "Cut"));
        assert!(!overlay_contains_label(&tree, "New"));
    }

    #[test]
    fn empty_menu_bar_builds() {
        let mut tree = WidgetTree::new().with_theme(Theme::light_default());
        let bar = tree.add(MenuBar::new());
        tree.layout(SizeProposal::exact(600.0, 30.0));
        assert!(tree.bounds(bar).height > 0.0);
    }

    #[test]
    fn trailing_slot_content() {
        use crate::Button;
        let mut tree = WidgetTree::new().with_theme(Theme::light_default());
        let bar = tree.add(
            MenuBar::new()
                .menu_literal("File", || {
                    Box::new(MenuList::new().item(MenuItem::new_literal("New").on_activate(TestCmd::New)))
                })
                .trailing_slot(Button::new_literal("Settings")),
        );
        tree.layout(SizeProposal::exact(600.0, 400.0));
        assert!(tree.bounds(bar).width > 0.0);
    }

    #[test]
    fn leading_slot_content() {
        let mut tree = WidgetTree::new().with_theme(Theme::light_default());
        let bar = tree.add(
            MenuBar::new()
                .leading_slot(TextWidget::new_literal("AppIcon"))
                .menu_literal("File", || {
                    Box::new(MenuList::new().item(MenuItem::new_literal("New").on_activate(TestCmd::New)))
                }),
        );
        tree.layout(SizeProposal::exact(600.0, 400.0));
        assert!(tree.bounds(bar).width > 0.0);
    }

    #[test]
    fn arrow_right_navigates_to_next_menu() {
        let (mut tree, bar) = setup_menu_bar();
        let file_center = trigger_center(&tree, bar, "File");

        // Open File menu
        tree.dispatch_event(WidgetEvent::PointerDown {
            position: file_center,
            button: fern_core::event::PointerButton::Primary,
            modifiers: fern_core::event::Modifiers::NONE,
        });
        tree.dispatch_event(WidgetEvent::PointerUp {
            position: file_center,
            button: fern_core::event::PointerButton::Primary,
            modifiers: fern_core::event::Modifiers::NONE,
        });
        tree.layout(SizeProposal::exact(600.0, 400.0));
        assert_eq!(tree.active_overlays().len(), 1);
        assert!(overlay_contains_label(&tree, "New"));

        // Press ArrowRight to navigate to Edit menu
        tree.press_key(Key::ArrowRight, fern_core::event::Modifiers::NONE);
        tree.layout(SizeProposal::exact(600.0, 400.0));

        assert_eq!(tree.active_overlays().len(), 1);
        assert!(overlay_contains_label(&tree, "Cut"));
        assert!(!overlay_contains_label(&tree, "New"));
    }

    #[test]
    fn arrow_left_navigates_to_previous_menu() {
        let (mut tree, bar) = setup_menu_bar();
        let edit_center = trigger_center(&tree, bar, "Edit");

        // Open Edit menu (index 1)
        tree.dispatch_event(WidgetEvent::PointerDown {
            position: edit_center,
            button: fern_core::event::PointerButton::Primary,
            modifiers: fern_core::event::Modifiers::NONE,
        });
        tree.dispatch_event(WidgetEvent::PointerUp {
            position: edit_center,
            button: fern_core::event::PointerButton::Primary,
            modifiers: fern_core::event::Modifiers::NONE,
        });
        tree.layout(SizeProposal::exact(600.0, 400.0));
        assert_eq!(tree.active_overlays().len(), 1);
        assert!(overlay_contains_label(&tree, "Cut"));

        // Press ArrowLeft to navigate to File menu
        tree.press_key(Key::ArrowLeft, fern_core::event::Modifiers::NONE);
        tree.layout(SizeProposal::exact(600.0, 400.0));

        assert_eq!(tree.active_overlays().len(), 1);
        assert!(overlay_contains_label(&tree, "New"));
        assert!(!overlay_contains_label(&tree, "Cut"));
    }

    #[test]
    fn escape_closes_menu_and_restores_focus() {
        let (mut tree, bar) = setup_menu_bar();
        let file_center = trigger_center(&tree, bar, "File");

        // Open File menu
        tree.dispatch_event(WidgetEvent::PointerDown {
            position: file_center,
            button: fern_core::event::PointerButton::Primary,
            modifiers: fern_core::event::Modifiers::NONE,
        });
        tree.dispatch_event(WidgetEvent::PointerUp {
            position: file_center,
            button: fern_core::event::PointerButton::Primary,
            modifiers: fern_core::event::Modifiers::NONE,
        });
        tree.layout(SizeProposal::exact(600.0, 400.0));
        assert_eq!(tree.active_overlays().len(), 1);

        // Press Escape to close menu
        tree.press_key(Key::Escape, fern_core::event::Modifiers::NONE);
        tree.layout(SizeProposal::exact(600.0, 400.0));

        assert_eq!(tree.active_overlays().len(), 0);

        tree.press_key(Key::Enter, fern_core::event::Modifiers::NONE);
        tree.layout(SizeProposal::exact(600.0, 400.0));

        assert_eq!(tree.active_overlays().len(), 1);
        assert!(overlay_contains_label(&tree, "New"));
    }

    #[test]
    fn focus_restored_when_menu_closes() {
        let (mut tree, bar) = setup_menu_bar();
        let file_center = trigger_center(&tree, bar, "File");

        // Open File menu
        tree.dispatch_event(WidgetEvent::PointerDown {
            position: file_center,
            button: fern_core::event::PointerButton::Primary,
            modifiers: fern_core::event::Modifiers::NONE,
        });
        tree.dispatch_event(WidgetEvent::PointerUp {
            position: file_center,
            button: fern_core::event::PointerButton::Primary,
            modifiers: fern_core::event::Modifiers::NONE,
        });
        tree.layout(SizeProposal::exact(600.0, 400.0));
        assert_eq!(tree.active_overlays().len(), 1);

        // Click outside to dismiss overlay
        tree.dispatch_event(WidgetEvent::PointerDown {
            position: fern_canvas::Point::new(1000.0, 1000.0),
            button: fern_core::event::PointerButton::Primary,
            modifiers: fern_core::event::Modifiers::NONE,
        });
        tree.layout(SizeProposal::exact(600.0, 400.0));

        assert_eq!(tree.active_overlays().len(), 0);

        tree.press_key(Key::Enter, fern_core::event::Modifiers::NONE);
        tree.layout(SizeProposal::exact(600.0, 400.0));

        assert_eq!(tree.active_overlays().len(), 1);
        assert!(overlay_contains_label(&tree, "New"));
    }

    #[test]
    fn click_menu_trigger_focuses_navigable_menu_list() {
        let (mut tree, bar) = setup_menu_bar();
        let file_center = trigger_center(&tree, bar, "File");
        let commands = capture_commands(&mut tree);

        // Open File menu by clicking trigger
        tree.dispatch_event(WidgetEvent::PointerDown {
            position: file_center,
            button: fern_core::event::PointerButton::Primary,
            modifiers: fern_core::event::Modifiers::NONE,
        });
        tree.dispatch_event(WidgetEvent::PointerUp {
            position: file_center,
            button: fern_core::event::PointerButton::Primary,
            modifiers: fern_core::event::Modifiers::NONE,
        });
        tree.layout(SizeProposal::exact(600.0, 400.0));

        assert_eq!(tree.active_overlays().len(), 1);

        tree.press_key(Key::ArrowDown, fern_core::event::Modifiers::NONE);
        tree.press_key(Key::Enter, fern_core::event::Modifiers::NONE);
        tree.layout(SizeProposal::exact(600.0, 400.0));

        assert_eq!(&*commands.borrow(), &[TestCmd::New]);
    }

    #[test]
    fn left_right_navigation_cycles_through_all_menus() {
        let (mut tree, bar) = setup_menu_bar();
        let file_center = trigger_center(&tree, bar, "File");

        // Start with File menu open
        tree.dispatch_event(WidgetEvent::PointerDown {
            position: file_center,
            button: fern_core::event::PointerButton::Primary,
            modifiers: fern_core::event::Modifiers::NONE,
        });
        tree.dispatch_event(WidgetEvent::PointerUp {
            position: file_center,
            button: fern_core::event::PointerButton::Primary,
            modifiers: fern_core::event::Modifiers::NONE,
        });
        tree.layout(SizeProposal::exact(600.0, 400.0));
        assert_eq!(tree.active_overlays().len(), 1);
        assert!(overlay_contains_label(&tree, "New"));

        // Navigate right to Edit menu
        tree.press_key(Key::ArrowRight, fern_core::event::Modifiers::NONE);
        tree.layout(SizeProposal::exact(600.0, 400.0));
        assert_eq!(tree.active_overlays().len(), 1);
        assert!(overlay_contains_label(&tree, "Cut"));

        // Navigate right again should go to View menu
        tree.press_key(Key::ArrowRight, fern_core::event::Modifiers::NONE);
        tree.layout(SizeProposal::exact(600.0, 400.0));
        assert_eq!(tree.active_overlays().len(), 1);
        assert!(overlay_contains_label(&tree, "Zoom In"));

        // Navigate right again should wrap to File menu
        tree.press_key(Key::ArrowRight, fern_core::event::Modifiers::NONE);
        tree.layout(SizeProposal::exact(600.0, 400.0));
        assert_eq!(tree.active_overlays().len(), 1);
        assert!(overlay_contains_label(&tree, "New"));

        // Navigate left should go to View menu
        tree.press_key(Key::ArrowLeft, fern_core::event::Modifiers::NONE);
        tree.layout(SizeProposal::exact(600.0, 400.0));
        assert_eq!(tree.active_overlays().len(), 1);
        assert!(overlay_contains_label(&tree, "Zoom In"));

        // Navigate left again should go to Edit menu
        tree.press_key(Key::ArrowLeft, fern_core::event::Modifiers::NONE);
        tree.layout(SizeProposal::exact(600.0, 400.0));
        assert_eq!(tree.active_overlays().len(), 1);
        assert!(overlay_contains_label(&tree, "Cut"));

        // Navigate left again should wrap to File menu
        tree.press_key(Key::ArrowLeft, fern_core::event::Modifiers::NONE);
        tree.layout(SizeProposal::exact(600.0, 400.0));
        assert_eq!(tree.active_overlays().len(), 1);
        assert!(overlay_contains_label(&tree, "New"));
    }

    #[test]
    fn up_down_arrows_work_in_menu_list_after_click() {
        let (mut tree, bar) = setup_menu_bar();
        let file_center = trigger_center(&tree, bar, "File");
        let commands = capture_commands(&mut tree);

        // Open File menu by clicking trigger
        tree.dispatch_event(WidgetEvent::PointerDown {
            position: file_center,
            button: fern_core::event::PointerButton::Primary,
            modifiers: fern_core::event::Modifiers::NONE,
        });
        tree.dispatch_event(WidgetEvent::PointerUp {
            position: file_center,
            button: fern_core::event::PointerButton::Primary,
            modifiers: fern_core::event::Modifiers::NONE,
        });
        tree.layout(SizeProposal::exact(600.0, 400.0));

        assert_eq!(tree.active_overlays().len(), 1);

        // ArrowDown should navigate to first item
        tree.press_key(Key::ArrowDown, fern_core::event::Modifiers::NONE);

        // ArrowDown should navigate to second item
        tree.press_key(Key::ArrowDown, fern_core::event::Modifiers::NONE);

        // ArrowUp should navigate back to first item
        tree.press_key(Key::ArrowUp, fern_core::event::Modifiers::NONE);
        tree.press_key(Key::Enter, fern_core::event::Modifiers::NONE);
        tree.layout(SizeProposal::exact(600.0, 400.0));

        assert_eq!(&*commands.borrow(), &[TestCmd::New]);
    }

    #[test]
    fn repeated_left_right_navigation_doesnt_stop() {
        let (mut tree, bar) = setup_menu_bar();
        let file_center = trigger_center(&tree, bar, "File");

        // Open File menu
        tree.dispatch_event(WidgetEvent::PointerDown {
            position: file_center,
            button: fern_core::event::PointerButton::Primary,
            modifiers: fern_core::event::Modifiers::NONE,
        });
        tree.dispatch_event(WidgetEvent::PointerUp {
            position: file_center,
            button: fern_core::event::PointerButton::Primary,
            modifiers: fern_core::event::Modifiers::NONE,
        });
        tree.layout(SizeProposal::exact(600.0, 400.0));

        // Navigate right multiple times - should keep working
        for _ in 0..10 {
            tree.press_key(Key::ArrowRight, fern_core::event::Modifiers::NONE);
            tree.layout(SizeProposal::exact(600.0, 400.0));
            assert_eq!(
                tree.active_overlays().len(),
                1,
                "Navigation should continue working"
            );
            assert!(
                overlay_contains_label(&tree, "New")
                    || overlay_contains_label(&tree, "Cut")
                    || overlay_contains_label(&tree, "Zoom In")
            );
        }

        // Navigate left multiple times - should keep working
        for _ in 0..10 {
            tree.press_key(Key::ArrowLeft, fern_core::event::Modifiers::NONE);
            tree.layout(SizeProposal::exact(600.0, 400.0));
            assert_eq!(
                tree.active_overlays().len(),
                1,
                "Navigation should continue working"
            );
            assert!(
                overlay_contains_label(&tree, "New")
                    || overlay_contains_label(&tree, "Cut")
                    || overlay_contains_label(&tree, "Zoom In")
            );
        }
    }

    #[test]
    fn menu_navigation_with_complex_layout_like_example() {
        let mut tree = WidgetTree::new().with_theme(Theme::light_default());

        let menu_bar = tree.add(
            MenuBar::new()
                .menu_literal("File", || {
                    Box::new(
                        MenuList::new()
                            .item(MenuItem::new_literal("New").on_activate(TestCmd::New))
                            .item(MenuItem::new_literal("Open").on_activate(TestCmd::Open)),
                    )
                })
                .menu_literal("Edit", || {
                    Box::new(
                        MenuList::new()
                            .item(MenuItem::new_literal("Cut").on_activate(TestCmd::Cut))
                            .item(MenuItem::new_literal("Copy").on_activate(TestCmd::Copy)),
                    )
                })
                .menu_literal("View", || {
                    Box::new(MenuList::new().item(MenuItem::new_literal("Zoom").on_activate(TestCmd::New)))
                }),
        );

        let toolbar = tree.add(TextWidget::new_literal("Toolbar"));
        let content = tree.add(TextWidget::new_literal("Content Area"));
        let status_bar = tree.add(TextWidget::new_literal("Status Bar"));

        let root = tree.add(
            VStack::new()
                .add_child(menu_bar)
                .add_child(toolbar)
                .child(Expand::new().fills_stack().set_child(content))
                .add_child(status_bar),
        );

        tree.layout(SizeProposal::exact(800.0, 600.0));

        // Click first menu (File)
        let first_trigger_center = trigger_center(&tree, root, "File");
        tree.dispatch_event(WidgetEvent::PointerDown {
            position: first_trigger_center,
            button: fern_core::event::PointerButton::Primary,
            modifiers: fern_core::event::Modifiers::NONE,
        });
        tree.dispatch_event(WidgetEvent::PointerUp {
            position: first_trigger_center,
            button: fern_core::event::PointerButton::Primary,
            modifiers: fern_core::event::Modifiers::NONE,
        });
        tree.layout(SizeProposal::exact(800.0, 600.0));

        assert_eq!(tree.active_overlays().len(), 1, "File menu should be open");
        assert!(overlay_contains_label(&tree, "New"));

        // Navigate to Edit menu
        tree.press_key(Key::ArrowRight, fern_core::event::Modifiers::NONE);
        tree.layout(SizeProposal::exact(800.0, 600.0));
        assert_eq!(tree.active_overlays().len(), 1, "Edit menu should be open");
        assert!(overlay_contains_label(&tree, "Cut"));

        // Navigate to View menu
        tree.press_key(Key::ArrowRight, fern_core::event::Modifiers::NONE);
        tree.layout(SizeProposal::exact(800.0, 600.0));
        assert_eq!(tree.active_overlays().len(), 1, "View menu should be open");
        assert!(overlay_contains_label(&tree, "Zoom"));

        // Navigate back to File menu (wrap around)
        tree.press_key(Key::ArrowRight, fern_core::event::Modifiers::NONE);
        tree.layout(SizeProposal::exact(800.0, 600.0));
        assert_eq!(
            tree.active_overlays().len(),
            1,
            "File menu should be open again"
        );
        assert!(overlay_contains_label(&tree, "New"));

        tree.press_key(Key::ArrowDown, fern_core::event::Modifiers::NONE);
        tree.layout(SizeProposal::exact(800.0, 600.0));
        assert_eq!(
            tree.active_overlays().len(),
            1,
            "File menu should remain open in the complex layout"
        );
        assert!(overlay_contains_label(&tree, "New"));
    }

    #[test]
    fn click_outside_clears_menu_state() {
        let (mut tree, bar) = setup_menu_bar();
        let file_center = trigger_center(&tree, bar, "File");
        let edit_center = trigger_center(&tree, bar, "Edit");

        tree.dispatch_event(WidgetEvent::PointerDown {
            position: file_center,
            button: fern_core::event::PointerButton::Primary,
            modifiers: fern_core::event::Modifiers::NONE,
        });
        tree.dispatch_event(WidgetEvent::PointerUp {
            position: file_center,
            button: fern_core::event::PointerButton::Primary,
            modifiers: fern_core::event::Modifiers::NONE,
        });
        tree.layout(SizeProposal::exact(600.0, 400.0));
        assert_eq!(tree.active_overlays().len(), 1);

        tree.dispatch_event(WidgetEvent::PointerDown {
            position: fern_canvas::Point::new(1000.0, 1000.0),
            button: fern_core::event::PointerButton::Primary,
            modifiers: fern_core::event::Modifiers::NONE,
        });
        tree.layout(SizeProposal::exact(600.0, 400.0));
        assert!(tree.active_overlays().is_empty());

        tree.pointer_move(edit_center);
        tree.layout(SizeProposal::exact(600.0, 400.0));
        assert!(
            tree.active_overlays().is_empty(),
            "hovering a trigger after dismissal should not reopen a menu"
        );
    }
}
