//! Milestone 4: Menus & Dropdowns
//!
//! Demonstrates the overlay-based interactive widgets added in Milestone 4:
//! - ComboBox with dropdown selection
//! - Context menu (right-click) with MenuList and MenuItem
//! - MenuItem with icons, shortcut labels, disabled items, separators
//! - Theme switching via context menu command
//!
//! Run with: `cargo run -p menus-and-dropdowns`

use std::cell::Cell;
use std::rc::Rc;

use fern_ui::prelude::*;
use fern_ui::widgets::{
    Button, ButtonStyle, ComboBox, Divider, Expand, HStack, IconWidget, MenuBar, MenuList,
    MenuItem, Padding, Panel, ScrollArea, Spacer, StatusBar, TextWidget, Toolbar, VStack,
};

// ---------------------------------------------------------------------------
// Application commands
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq)]
enum Cmd {
    ToggleDarkMode,
    Cut,
    Copy,
    Paste,
    SelectAll,
    Undo,
    Redo,
    NewFile,
    OpenFile,
    SaveFile,
    AlignLeft,
    AlignCenter,
    AlignRight,
    AlignJustify,
}

impl AppCommand for Cmd {}

// ---------------------------------------------------------------------------
// Root composite
// ---------------------------------------------------------------------------

#[derive(Debug)]
struct Root {
    root_child_id: Option<WidgetId>,
}

impl Root {
    fn new() -> Self {
        Self {
            root_child_id: None,
        }
    }
}

impl Widget for Root {
    fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
        let theme = ctx.theme().clone();
        let t = &theme.typography;
        let c = &theme.colors;

        // --- Section 1: ComboBox demos ---

        let fruit_selected = ctx.signal(None::<usize>);
        let color_selected = ctx.signal(Some(2_usize)); // pre-select "Blue"
        let size_selected = ctx.signal(None::<usize>);

        let combo_section = ctx.add(
            VStack::new()
                .spacing(12.0)
                .child(
                    TextWidget::new("ComboBox / Dropdown")
                        .style(t.heading_2.clone())
                        .color(c.on_surface),
                )
                .child(
                    TextWidget::new(
                        "Click to open the dropdown. Use arrow keys to navigate, \
                         Enter to select, Escape to close.",
                    )
                    .style(t.body.clone())
                    .color(c.on_surface),
                )
                .child(
                    HStack::new()
                        .spacing(16.0)
                        .child(
                            VStack::new()
                                .spacing(4.0)
                                .child(
                                    TextWidget::new("Fruit")
                                        .style(t.label.clone())
                                        .color(c.on_surface),
                                )
                                .child(
                                    ComboBox::new(
                                        vec![
                                            "Apple",
                                            "Banana",
                                            "Cherry",
                                            "Date",
                                            "Elderberry",
                                            "Fig",
                                            "Grape",
                                        ],
                                        fruit_selected.clone(),
                                    )
                                    .placeholder("Select a fruit..."),
                                ),
                        )
                        .child(
                            VStack::new()
                                .spacing(4.0)
                                .child(
                                    TextWidget::new("Color")
                                        .style(t.label.clone())
                                        .color(c.on_surface),
                                )
                                .child(ComboBox::new(
                                    vec!["Red", "Green", "Blue", "Yellow", "Purple"],
                                    color_selected.clone(),
                                )),
                        )
                        .child(
                            VStack::new()
                                .spacing(4.0)
                                .child(
                                    TextWidget::new("Size (disabled)")
                                        .style(t.label.clone())
                                        .color(c.on_surface),
                                )
                                .child(
                                    ComboBox::new(
                                        vec!["Small", "Medium", "Large"],
                                        size_selected.clone(),
                                    )
                                    .placeholder("Choose size")
                                    .enabled(false),
                                ),
                        ),
                ),
        );

        // --- Section 2: Context menu demo ---

        let context_menu_section = ctx.add(
            VStack::new()
                .spacing(12.0)
                .child(
                    TextWidget::new("Context Menu")
                        .style(t.heading_2.clone())
                        .color(c.on_surface),
                )
                .child(
                    TextWidget::new(
                        "Right-click on the panels below to open a context menu. \
                         Each panel has a different menu.",
                    )
                    .style(t.body.clone())
                    .color(c.on_surface),
                )
                .child(
                    HStack::new()
                        .spacing(16.0)
                        .child(
                            Panel::new()
                                .background(c.surface_secondary)
                                .corner_radius(8.0)
                                .padding(20.0)
                                .child(
                                    VStack::new()
                                        .spacing(6.0)
                                        .child(
                                            TextWidget::new("Edit Menu")
                                                .style(t.label.clone())
                                                .color(c.on_surface),
                                        )
                                        .child(
                                            TextWidget::new("Right-click for Cut/Copy/Paste")
                                                .style(t.body.clone())
                                                .color(c.on_surface),
                                        ),
                                )
                                .context_menu(|| {
                                    Box::new(
                                        MenuList::new()
                                            .item(
                                                MenuItem::new("Undo")
                                                    .on_activate(Cmd::Undo)
                                                    .shortcut_label("Ctrl+Z"),
                                            )
                                            .item(
                                                MenuItem::new("Redo")
                                                    .on_activate(Cmd::Redo)
                                                    .shortcut_label("Ctrl+Shift+Z"),
                                            )
                                            .separator()
                                            .item(
                                                MenuItem::new("Cut")
                                                    .on_activate(Cmd::Cut)
                                                    .shortcut_label("Ctrl+X"),
                                            )
                                            .item(
                                                MenuItem::new("Copy")
                                                    .on_activate(Cmd::Copy)
                                                    .shortcut_label("Ctrl+C"),
                                            )
                                            .item(
                                                MenuItem::new("Paste")
                                                    .on_activate(Cmd::Paste)
                                                    .shortcut_label("Ctrl+V"),
                                            )
                                            .separator()
                                            .item(
                                                MenuItem::new("Select All")
                                                    .on_activate(Cmd::SelectAll)
                                                    .shortcut_label("Ctrl+A"),
                                            )
                                            .separator()
                                            .item(MenuItem::submenu("Alignment", || {
                                                Box::new(
                                                    MenuList::new()
                                                        .item(MenuItem::new("Left").on_activate(Cmd::AlignLeft))
                                                        .item(MenuItem::new("Center").on_activate(Cmd::AlignCenter))
                                                        .item(MenuItem::new("Right").on_activate(Cmd::AlignRight))
                                                        .item(MenuItem::new("Justify").on_activate(Cmd::AlignJustify)),
                                                )
                                            })),
                                    )
                                }),
                        )
                        .child(
                            Panel::new()
                                .background(c.surface_secondary)
                                .corner_radius(8.0)
                                .padding(20.0)
                                .child(
                                    VStack::new()
                                        .spacing(6.0)
                                        .child(
                                            TextWidget::new("File Menu")
                                                .style(t.label.clone())
                                                .color(c.on_surface),
                                        )
                                        .child(
                                            TextWidget::new("Right-click for file operations")
                                                .style(t.body.clone())
                                                .color(c.on_surface),
                                        ),
                                )
                                .context_menu(|| {
                                    Box::new(
                                        MenuList::new()
                                            .item(
                                                MenuItem::new("New File")
                                                    .on_activate(Cmd::NewFile)
                                                    .shortcut_label("Ctrl+N"),
                                            )
                                            .item(
                                                MenuItem::new("Open File...")
                                                    .on_activate(Cmd::OpenFile)
                                                    .shortcut_label("Ctrl+O"),
                                            )
                                            .item(
                                                MenuItem::new("Save")
                                                    .on_activate(Cmd::SaveFile)
                                                    .shortcut_label("Ctrl+S"),
                                            )
                                            .separator()
                                            .item(MenuItem::new("Export as PDF").enabled(false)),
                                    )
                                }),
                        ),
                ),
        );

        // --- Section 3: MenuItem showcase ---

        let menu_showcase_section = ctx.add(
            VStack::new()
                .spacing(12.0)
                .child(
                    TextWidget::new("Menu Items (inline)")
                        .style(t.heading_2.clone())
                        .color(c.on_surface),
                )
                .child(
                    TextWidget::new(
                        "MenuItems shown directly (not in an overlay) to demonstrate \
                         their visual styles and interaction states.",
                    )
                    .style(t.body.clone())
                    .color(c.on_surface),
                )
                .child(
                    Panel::new()
                        .background(c.surface)
                        .border_color(c.border)
                        .border_width(1.0)
                        .corner_radius(8.0)
                        .child(
                            VStack::new()
                                .child(
                                    MenuItem::new("Normal item")
                                        .on_activate(Cmd::Cut)
                                        .shortcut_label("Ctrl+X"),
                                )
                                .child(
                                    MenuItem::new("With icon")
                                        .on_activate(Cmd::Copy)
                                        .icon(IconWidget::checkmark(16.0))
                                        .shortcut_label("Ctrl+C"),
                                )
                                .child(MenuItem::new("Disabled item").enabled(false))
                                .child(MenuItem::submenu("Submenu trigger", || {
                                    Box::new(TextWidget::new("submenu placeholder"))
                                })),
                        ),
                ),
        );

        // --- Assemble ---

        let toolbar = ctx.add(
            Toolbar::new().child(
                HStack::new()
                    .child(
                        TextWidget::new("Menus & Dropdowns")
                            .style(t.heading_1.clone())
                            .color(c.on_surface),
                    )
                    .child(Spacer::new())
                    .child(
                        Button::new("Toggle Dark Mode")
                            .style(ButtonStyle::Outlined)
                            .on_click(Cmd::ToggleDarkMode),
                    ),
            ),
        );

        let content = ctx.add(
            VStack::new()
                .spacing(32.0)
                .add_child(combo_section)
                .child(Divider::new().thickness(2.0))
                .add_child(context_menu_section)
                .child(Divider::new().thickness(2.0))
                .add_child(menu_showcase_section),
        );
        let padded = ctx.add(Padding::uniform(24.0).set_child(content));
        let scroll = ctx.add(ScrollArea::from_id(padded).scroll_bar_style(fern_ui::widgets::ScrollBarStyle::Overlay));

        let menu_bar = ctx.add(
            MenuBar::new()
                .leading_slot(
                    IconWidget::chevron_right(16.0)
                        .color(c.primary),
                )
                .menu("File", || {
                    Box::new(
                        MenuList::new()
                            .item(
                                MenuItem::new("New")
                                    .on_activate(Cmd::NewFile)
                                    .shortcut_label("Ctrl+N"),
                            )
                            .item(
                                MenuItem::new("Open")
                                    .on_activate(Cmd::OpenFile)
                                    .shortcut_label("Ctrl+O"),
                            )
                            .item(
                                MenuItem::new("Save")
                                    .on_activate(Cmd::SaveFile)
                                    .shortcut_label("Ctrl+S"),
                            )
                            .separator()
                            .item(MenuItem::new("Quit").on_activate(Cmd::ToggleDarkMode)),
                    )
                })
                .menu("Edit", || {
                    Box::new(
                        MenuList::new()
                            .item(
                                MenuItem::new("Undo")
                                    .on_activate(Cmd::Undo)
                                    .shortcut_label("Ctrl+Z"),
                            )
                            .item(
                                MenuItem::new("Redo")
                                    .on_activate(Cmd::Redo)
                                    .shortcut_label("Ctrl+Shift+Z"),
                            )
                            .separator()
                            .item(
                                MenuItem::new("Cut")
                                    .on_activate(Cmd::Cut)
                                    .shortcut_label("Ctrl+X"),
                            )
                            .item(
                                MenuItem::new("Copy")
                                    .on_activate(Cmd::Copy)
                                    .shortcut_label("Ctrl+C"),
                            )
                            .item(
                                MenuItem::new("Paste")
                                    .on_activate(Cmd::Paste)
                                    .shortcut_label("Ctrl+V"),
                            )
                            .separator()
                            .item(
                                MenuItem::new("Select All")
                                    .on_activate(Cmd::SelectAll)
                                    .shortcut_label("Ctrl+A"),
                            ),
                    )
                })
                .menu("View", || {
                    Box::new(
                        MenuList::new()
                            .item(MenuItem::submenu("Alignment", || {
                                Box::new(
                                    MenuList::new()
                                        .item(MenuItem::new("Left").on_activate(Cmd::AlignLeft))
                                        .item(
                                            MenuItem::new("Center")
                                                .on_activate(Cmd::AlignCenter),
                                        )
                                        .item(
                                            MenuItem::new("Right").on_activate(Cmd::AlignRight),
                                        )
                                        .item(
                                            MenuItem::new("Justify")
                                                .on_activate(Cmd::AlignJustify),
                                        ),
                                )
                            }))
                            .separator()
                            .item(MenuItem::new("Toggle Dark Mode").on_activate(Cmd::ToggleDarkMode)),
                    )
                })
                .trailing_slot(
                    Button::new("Settings")
                        .style(ButtonStyle::Flat)
                        .on_click(Cmd::ToggleDarkMode),
                ),
        );

        let root = ctx.add(
            VStack::new()
                .add_child(menu_bar)
                .add_child(toolbar)
                .child(Expand::new().fills_stack().set_child(scroll))
                .child(
                    StatusBar::new().child(
                        TextWidget::new("Milestone 4 -- Menus & Dropdowns")
                            .style(t.caption.clone())
                            .color(c.on_surface),
                    ),
                ),
        );

        self.root_child_id = Some(root);
        vec![root]
    }

    fn size_that_fits(&self, proposal: SizeProposal, ctx: &LayoutContext) -> Size {
        match self.root_child_id {
            Some(id) => ctx
                .child_size(id, proposal)
                .unwrap_or_else(|| proposal.resolve(0.0, 0.0)),
            None => proposal.resolve(0.0, 0.0),
        }
    }
}

// ---------------------------------------------------------------------------
// Entry point
// ---------------------------------------------------------------------------

fn main() {
    let is_dark = Rc::new(Cell::new(false));
    let is_dark_clone = is_dark.clone();

    FernAppBuilder::new()
        .theme(Theme::light_default())
        .window_title("FernUI — Menus & Dropdowns (Milestone 4)")
        .window_size(900, 700)
        .on_command(move |cmd: &Cmd, ctx| match cmd {
            Cmd::ToggleDarkMode => {
                let dark = !is_dark_clone.get();
                is_dark_clone.set(dark);
                println!("Theme: {}", if dark { "dark" } else { "light" });
                if dark {
                    ctx.set_theme(Theme::dark_default());
                } else {
                    ctx.set_theme(Theme::light_default());
                }
            }
            other => println!("Command: {:?}", other),
        })
        .root(|tree| tree.add(Root::new()))
        .run();
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use fern_ui::core::signal::Signal;
    use fern_ui::core::WidgetTree;
    use fern_ui::prelude::*;
    use fern_ui::widgets::{ComboBox, MenuList, MenuItem, MenuSeparator};

    #[derive(Debug, Clone, PartialEq)]
    enum TestCmd {
        Action,
    }
    impl AppCommand for TestCmd {}

    #[test]
    fn root_composite_builds_and_lays_out() {
        let mut tree = WidgetTree::new().with_theme(Theme::light_default());
        let root = tree.add(super::Root::new());
        tree.layout(SizeProposal::exact(900.0, 700.0));
        let bounds = tree.bounds(root);
        assert!(bounds.width > 0.0);
        assert!(bounds.height > 0.0);
    }

    #[test]
    fn combo_box_opens_on_click() {
        let mut tree = WidgetTree::new().with_theme(Theme::light_default());
        let selected = Signal::new(None::<usize>);
        let cb = tree.add(
            ComboBox::new(vec!["Apple", "Banana", "Cherry"], selected.clone())
                .placeholder("Pick one"),
        );
        tree.layout(SizeProposal::exact(300.0, 200.0));

        assert!(tree.active_overlays().is_empty());
        tree.click(cb);
        tree.layout(SizeProposal::exact(300.0, 200.0));
        assert_eq!(tree.active_overlays().len(), 1, "dropdown should be open");
    }

    #[test]
    fn combo_box_arrow_keys_change_selection() {
        let mut tree = WidgetTree::new().with_theme(Theme::light_default());
        let selected = Signal::new(None::<usize>);
        let cb = tree.add(ComboBox::new(vec!["A", "B", "C"], selected.clone()));
        tree.layout(SizeProposal::exact(300.0, 200.0));
        tree.focus(cb);

        tree.press_key(Key::ArrowDown, Modifiers::NONE);
        assert_eq!(selected.get(), Some(1));

        tree.press_key(Key::ArrowUp, Modifiers::NONE);
        assert_eq!(selected.get(), Some(0));
    }

    #[test]
    fn menu_list_builds_with_items_and_separators() {
        let mut tree = WidgetTree::new().with_theme(Theme::light_default());
        let menu = tree.add(
            MenuList::new()
                .item(MenuItem::new("Cut").on_activate(TestCmd::Action))
                .separator()
                .item(MenuItem::new("Copy").on_activate(TestCmd::Action)),
        );
        tree.layout(SizeProposal::exact(200.0, 200.0));
        let bounds = tree.bounds(menu);
        assert!(bounds.width >= 120.0);
        assert!(bounds.height > 0.0);
    }

    #[test]
    fn menu_item_tap_emits_command() {
        let mut tree = WidgetTree::new().with_theme(Theme::light_default());
        let item = tree.add(MenuItem::new("Cut").on_activate(TestCmd::Action));
        tree.layout(SizeProposal::exact(200.0, 40.0));

        let called = std::rc::Rc::new(std::cell::Cell::new(false));
        let c = called.clone();
        tree.on_command(move |cmd: &TestCmd| {
            if *cmd == TestCmd::Action {
                c.set(true);
            }
        });

        tree.click(item);
        assert!(called.get(), "tap on menu item should emit command");
    }

    #[test]
    fn menu_item_disabled_ignores_tap() {
        let mut tree = WidgetTree::new().with_theme(Theme::light_default());
        let item = tree.add(MenuItem::new("Nope").on_activate(TestCmd::Action).enabled(false));
        tree.layout(SizeProposal::exact(200.0, 40.0));

        let called = std::rc::Rc::new(std::cell::Cell::new(false));
        let c = called.clone();
        tree.on_command(move |_cmd: &TestCmd| c.set(true));

        tree.click(item);
        assert!(!called.get(), "disabled item should not emit command");
    }

    #[test]
    fn context_menu_opens_on_right_click() {
        use fern_ui::core::event::PointerButton;
        use fern_ui::widgets::Panel;

        let mut tree = WidgetTree::new().with_theme(Theme::light_default());
        let panel = tree.add(
            Panel::new()
                .padding(20.0)
                .child(fern_ui::widgets::TextWidget::new("Right-click me"))
                .context_menu(|| {
                    Box::new(
                        MenuList::new()
                            .item(MenuItem::new("Action").on_activate(TestCmd::Action)),
                    )
                }),
        );
        tree.layout(SizeProposal::exact(300.0, 100.0));

        assert!(tree.active_overlays().is_empty());

        let center = tree.bounds(panel).center();
        tree.pointer_down_button(center, PointerButton::Secondary);
        tree.layout(SizeProposal::exact(300.0, 100.0));

        assert_eq!(
            tree.active_overlays().len(),
            1,
            "right-click should open context menu overlay"
        );
    }

    #[test]
    fn escape_dismisses_overlay() {
        use fern_ui::core::event::PointerButton;
        use fern_ui::widgets::Panel;

        let mut tree = WidgetTree::new().with_theme(Theme::light_default());
        let panel = tree.add(
            Panel::new()
                .padding(20.0)
                .child(fern_ui::widgets::TextWidget::new("Right-click me"))
                .context_menu(|| {
                    Box::new(
                        MenuList::new()
                            .item(MenuItem::new("Action").on_activate(TestCmd::Action)),
                    )
                }),
        );
        tree.layout(SizeProposal::exact(300.0, 100.0));

        // Open context menu
        let center = tree.bounds(panel).center();
        tree.pointer_down_button(center, PointerButton::Secondary);
        tree.layout(SizeProposal::exact(300.0, 100.0));
        assert_eq!(tree.active_overlays().len(), 1);

        // Escape should dismiss
        tree.press_key(Key::Escape, Modifiers::NONE);
        assert!(
            tree.active_overlays().is_empty(),
            "Escape should dismiss the overlay"
        );
    }

    #[test]
    fn theme_switch_rebuilds_correctly() {
        let mut tree = WidgetTree::new().with_theme(Theme::light_default());
        let root = tree.add(super::Root::new());
        tree.layout(SizeProposal::exact(900.0, 700.0));
        let light_frame = tree.render();

        tree.set_theme(Theme::dark_default());
        tree.layout(SizeProposal::exact(900.0, 700.0));
        let dark_frame = tree.render();

        assert_ne!(
            light_frame.shapes, dark_frame.shapes,
            "theme switch should change rendered output"
        );
    }

    #[test]
    fn context_menu_overlay_has_correct_bounds() {
        use fern_ui::core::event::PointerButton;
        use fern_ui::widgets::Panel;

        let mut tree = WidgetTree::new().with_theme(Theme::light_default());
        let panel = tree.add(
            Panel::new()
                .padding(20.0)
                .child(fern_ui::widgets::TextWidget::new("Right-click me"))
                .context_menu(|| {
                    Box::new(
                        MenuList::new()
                            .item(MenuItem::new("Cut").on_activate(TestCmd::Action))
                            .separator()
                            .item(MenuItem::new("Copy").on_activate(TestCmd::Action))
                            .item(MenuItem::new("Paste").on_activate(TestCmd::Action)),
                    )
                }),
        );
        tree.layout(SizeProposal::exact(400.0, 300.0));

        let center = tree.bounds(panel).center();
        tree.pointer_down_button(center, PointerButton::Secondary);
        tree.layout(SizeProposal::exact(400.0, 300.0));

        let overlays = tree.active_overlays();
        assert_eq!(overlays.len(), 1);

        // The overlay content should have reasonable bounds, NOT fill the page
        let content_ids = tree.overlay_manager().active_content_ids();
        let content_bounds = tree.bounds(content_ids[0]);
        eprintln!(
            "Overlay content bounds: x={}, y={}, w={}, h={}",
            content_bounds.x, content_bounds.y, content_bounds.width, content_bounds.height
        );
        assert!(
            content_bounds.width < 350.0,
            "menu should not fill the page width (got {})",
            content_bounds.width
        );
        assert!(
            content_bounds.height < 250.0,
            "menu should not fill the page height (got {})",
            content_bounds.height
        );
        assert!(
            content_bounds.width >= 120.0,
            "menu should have minimum width (got {})",
            content_bounds.width
        );
        assert!(
            content_bounds.height > 30.0,
            "menu should have some height for items (got {})",
            content_bounds.height
        );
    }

    #[test]
    fn submenu_opens_on_hover_after_delay() {
        use std::time::Duration;

        let mut tree = WidgetTree::new().with_theme(Theme::light_default());
        let item = tree.add(
            MenuItem::submenu("Open Recent", || {
                Box::new(
                    MenuList::new()
                        .item(MenuItem::new("File 1").on_activate(TestCmd::Action))
                        .item(MenuItem::new("File 2").on_activate(TestCmd::Action)),
                )
            })
            .submenu_delay(Duration::from_millis(100)),
        );
        tree.layout(SizeProposal::exact(200.0, 40.0));

        assert!(tree.active_overlays().is_empty());

        // Hover over the submenu trigger
        let center = tree.bounds(item).center();
        tree.pointer_move(center);

        // Advance past the delay — widget tree processes pending overlays
        tree.advance_time(Duration::from_millis(150));

        assert_eq!(
            tree.active_overlays().len(),
            1,
            "hovering on submenu item should open submenu overlay after delay"
        );
    }

    #[test]
    fn click_outside_dismisses_context_menu() {
        use fern_ui::core::event::PointerButton;
        use fern_ui::widgets::Panel;

        let mut tree = WidgetTree::new().with_theme(Theme::light_default());
        let panel = tree.add(
            Panel::new()
                .padding(20.0)
                .child(fern_ui::widgets::TextWidget::new("Right-click me"))
                .context_menu(|| {
                    Box::new(
                        MenuList::new()
                            .item(MenuItem::new("Action").on_activate(TestCmd::Action)),
                    )
                }),
        );
        tree.layout(SizeProposal::exact(400.0, 300.0));

        // Open context menu
        let center = tree.bounds(panel).center();
        tree.pointer_down_button(center, PointerButton::Secondary);
        tree.layout(SizeProposal::exact(400.0, 300.0));
        assert_eq!(tree.active_overlays().len(), 1);

        // Click far outside the menu
        tree.dispatch_event(WidgetEvent::PointerDown {
            position: fern_ui::prelude::Point::new(399.0, 299.0),
            button: PointerButton::Primary,
        });
        assert!(
            tree.active_overlays().is_empty(),
            "clicking outside should dismiss context menu"
        );
    }

    #[test]
    fn menu_item_activation_dismisses_overlay() {
        use fern_ui::core::event::PointerButton;
        use fern_ui::widgets::Panel;

        let mut tree = WidgetTree::new().with_theme(Theme::light_default());
        let panel = tree.add(
            Panel::new()
                .padding(20.0)
                .child(fern_ui::widgets::TextWidget::new("Right-click me"))
                .context_menu(|| {
                    Box::new(
                        MenuList::new()
                            .item(MenuItem::new("Action").on_activate(TestCmd::Action)),
                    )
                }),
        );
        tree.layout(SizeProposal::exact(400.0, 300.0));

        // Open context menu
        let center = tree.bounds(panel).center();
        tree.pointer_down_button(center, PointerButton::Secondary);
        tree.layout(SizeProposal::exact(400.0, 300.0));
        assert_eq!(tree.active_overlays().len(), 1, "menu should be open");

        // Click the menu item — find it via overlay content
        let content_ids = tree.overlay_manager().active_content_ids();
        tree.click(content_ids[0]);
        assert!(
            tree.active_overlays().is_empty(),
            "menu should be dismissed after item activation"
        );
    }

    #[test]
    fn right_click_replaces_context_menu() {
        use fern_ui::core::event::PointerButton;
        use fern_ui::widgets::Panel;

        let mut tree = WidgetTree::new().with_theme(Theme::light_default());
        let panel = tree.add(
            Panel::new()
                .padding(20.0)
                .child(fern_ui::widgets::TextWidget::new("Right-click me"))
                .context_menu(|| {
                    Box::new(
                        MenuList::new()
                            .item(MenuItem::new("Action").on_activate(TestCmd::Action)),
                    )
                }),
        );
        tree.layout(SizeProposal::exact(400.0, 300.0));

        // Open context menu at one position
        let center = tree.bounds(panel).center();
        tree.pointer_down_button(center, PointerButton::Secondary);
        tree.layout(SizeProposal::exact(400.0, 300.0));
        assert_eq!(tree.active_overlays().len(), 1);

        // Right-click elsewhere on the same panel — should replace, not stack
        let other = fern_ui::prelude::Point::new(center.x + 20.0, center.y + 20.0);
        tree.pointer_down_button(other, PointerButton::Secondary);
        tree.layout(SizeProposal::exact(400.0, 300.0));
        assert_eq!(
            tree.active_overlays().len(),
            1,
            "right-click should replace existing context menu, not stack"
        );
    }

    #[test]
    fn shortcut_display() {
        use fern_ui::prelude::Shortcut;

        assert_eq!(Shortcut::ctrl(Key::S).to_string(), "Ctrl+S");
        assert_eq!(Shortcut::ctrl_shift(Key::Z).to_string(), "Ctrl+Shift+Z");
        assert_eq!(Shortcut::alt(Key::F4).to_string(), "Alt+F4");
    }
}
