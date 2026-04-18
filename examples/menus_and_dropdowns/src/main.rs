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
    Button, ButtonVariant, ComboBox, Divider, Expand, HStack, IconWidget, MenuBar, MenuItem,
    MenuList, Padding, Panel, ScrollArea, Spacer, StatusBar, TextWidget, Toolbar, VStack,
};

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
        let theme = ctx.theme_signal().get();
        let t = &theme.typography;
        let c = &theme.colors;

        // --- Section 1: ComboBox demos ---

        let fruit_selected = ctx.signal(None::<String>);
        let color_selected = ctx.signal(Some("Blue".to_string())); // pre-select "Blue"
        let size_selected = ctx.signal(None::<String>);
        // Searchable combo — its query signal is held externally so the
        // caller could observe or clear it. Here we just let it live
        // alongside the selection signal.
        let country_selected = ctx.signal(None::<String>);
        let country_query = ctx.signal(String::new());

        let combo_section = ctx.add(
            VStack::new()
                .spacing(12.0)
                .child(
                    TextWidget::new_literal("ComboBox / Dropdown")
                        .style(t.body_bold.clone())
                        .color(TextRole::Primary),
                )
                .child(
                    TextWidget::new_literal(
                        "Click to open the dropdown. Use arrow keys to navigate, \
                         Enter to select, Escape to close. The searchable variant \
                         adds a text field at the top of the panel that filters the \
                         list live.",
                    )
                    .style(t.body.clone())
                    .color(TextRole::Primary),
                )
                .child(
                    HStack::new()
                        .spacing(16.0)
                        .child(
                            VStack::new()
                                .spacing(4.0)
                                .child(
                                    TextWidget::new_literal("Fruit")
                                        .style(t.small.clone())
                                        .color(TextRole::Primary),
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
                                    TextWidget::new_literal("Color")
                                        .style(t.small.clone())
                                        .color(TextRole::Primary),
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
                                    TextWidget::new_literal("Size (disabled)")
                                        .style(t.small.clone())
                                        .color(TextRole::Primary),
                                )
                                .child(
                                    ComboBox::new(
                                        vec!["Small", "Medium", "Large"],
                                        size_selected.clone(),
                                    )
                                    .placeholder("Choose size")
                                    .enabled(false),
                                ),
                        )
                        .child(
                            VStack::new()
                                .spacing(4.0)
                                .child(
                                    TextWidget::new_literal("Country (searchable)")
                                        .style(t.small.clone())
                                        .color(TextRole::Primary),
                                )
                                .child(
                                    ComboBox::new(
                                        vec![
                                            "Argentina",
                                            "Australia",
                                            "Belgium",
                                            "Brazil",
                                            "Canada",
                                            "Chile",
                                            "China",
                                            "Denmark",
                                            "Egypt",
                                            "Finland",
                                            "France",
                                            "Germany",
                                            "Greece",
                                            "Iceland",
                                            "India",
                                            "Ireland",
                                            "Italy",
                                            "Japan",
                                            "Mexico",
                                            "Netherlands",
                                            "Norway",
                                            "Poland",
                                            "Portugal",
                                            "Spain",
                                            "Sweden",
                                            "Switzerland",
                                            "Turkey",
                                            "United Kingdom",
                                            "United States",
                                            "Vietnam",
                                        ],
                                        country_selected.clone(),
                                    )
                                    .placeholder("Pick a country…")
                                    .search_query(country_query.clone())
                                    .max_visible_items(6),
                                ),
                        ),
                ),
        );

        // --- Section 2: Context menu demo ---

        let context_menu_section = ctx.add(
            VStack::new()
                .spacing(12.0)
                .child(
                    TextWidget::new_literal("Context Menu")
                        .style(t.body_bold.clone())
                        .color(TextRole::Primary),
                )
                .child(
                    TextWidget::new_literal(
                        "Right-click on the panels below to open a context menu. \
                         Each panel has a different menu.",
                    )
                    .style(t.body.clone())
                    .color(TextRole::Primary),
                )
                .child(
                    HStack::new()
                        .spacing(16.0)
                        .child(
                            Panel::new()
                                .background(SurfaceRole::Main)
                                .corner_radius(8.0)
                                .padding(20.0)
                                .child(
                                    VStack::new()
                                        .spacing(6.0)
                                        .child(
                                            TextWidget::new_literal("Edit Menu")
                                                .style(t.small.clone())
                                                .color(TextRole::Primary),
                                        )
                                        .child(
                                            TextWidget::new_literal("Right-click for Cut/Copy/Paste")
                                                .style(t.body.clone())
                                                .color(TextRole::Primary),
                                        ),
                                )
                                .context_menu(|| {
                                    Box::new(
                                        MenuList::new()
                                            .item(
                                                MenuItem::new_literal("Undo")
                                                    .on_activate_fn(|_| println!("Undo"))
                                                    .shortcut_label("Ctrl+Z"),
                                            )
                                            .item(
                                                MenuItem::new_literal("Redo")
                                                    .on_activate_fn(|_| println!("Redo"))
                                                    .shortcut_label("Ctrl+Shift+Z"),
                                            )
                                            .separator()
                                            .item(
                                                MenuItem::new_literal("Cut")
                                                    .on_activate_fn(|_| println!("Cut"))
                                                    .shortcut_label("Ctrl+X"),
                                            )
                                            .item(
                                                MenuItem::new_literal("Copy")
                                                    .on_activate_fn(|_| println!("Copy"))
                                                    .shortcut_label("Ctrl+C"),
                                            )
                                            .item(
                                                MenuItem::new_literal("Paste")
                                                    .on_activate_fn(|_| println!("Paste"))
                                                    .shortcut_label("Ctrl+V"),
                                            )
                                            .separator()
                                            .item(
                                                MenuItem::new_literal("Select All")
                                                    .on_activate_fn(|_| println!("SelectAll"))
                                                    .shortcut_label("Ctrl+A"),
                                            )
                                            .separator()
                                            .item(MenuItem::submenu_literal("Alignment", || {
                                                Box::new(
                                                    MenuList::new()
                                                        .item(
                                                            MenuItem::new_literal("Left")
                                                                .on_activate_fn(|_| println!("AlignLeft")),
                                                        )
                                                        .item(
                                                            MenuItem::new_literal("Center")
                                                                .on_activate_fn(|_| println!("AlignCenter")),
                                                        )
                                                        .item(
                                                            MenuItem::new_literal("Right")
                                                                .on_activate_fn(|_| println!("AlignRight")),
                                                        )
                                                        .item(
                                                            MenuItem::new_literal("Justify")
                                                                .on_activate_fn(|_| println!("AlignJustify")),
                                                        ),
                                                )
                                            })),
                                    )
                                }),
                        )
                        .child(
                            Panel::new()
                                .background(SurfaceRole::Main)
                                .corner_radius(8.0)
                                .padding(20.0)
                                .child(
                                    VStack::new()
                                        .spacing(6.0)
                                        .child(
                                            TextWidget::new_literal("File Menu")
                                                .style(t.small.clone())
                                                .color(TextRole::Primary),
                                        )
                                        .child(
                                            TextWidget::new_literal("Right-click for file operations")
                                                .style(t.body.clone())
                                                .color(TextRole::Primary),
                                        ),
                                )
                                .context_menu(|| {
                                    Box::new(
                                        MenuList::new()
                                            .item(
                                                MenuItem::new_literal("New File")
                                                    .on_activate_fn(|_| println!("NewFile"))
                                                    .shortcut_label("Ctrl+N"),
                                            )
                                            .item(
                                                MenuItem::new_literal("Open File...")
                                                    .on_activate_fn(|_| println!("OpenFile"))
                                                    .shortcut_label("Ctrl+O"),
                                            )
                                            .item(
                                                MenuItem::new_literal("Save")
                                                    .on_activate_fn(|_| println!("SaveFile"))
                                                    .shortcut_label("Ctrl+S"),
                                            )
                                            .separator()
                                            .item(MenuItem::new_literal("Export as PDF").enabled(false)),
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
                    TextWidget::new_literal("Menu Items (inline)")
                        .style(t.body_bold.clone())
                        .color(TextRole::Primary),
                )
                .child(
                    TextWidget::new_literal(
                        "MenuItems shown directly (not in an overlay) to demonstrate \
                         their visual styles and interaction states.",
                    )
                    .style(t.body.clone())
                    .color(TextRole::Primary),
                )
                .child(
                    Panel::new()
                        .background(SurfaceRole::Main)
                        .border_color(BorderRole::Default)
                        .border_width(1.0)
                        .corner_radius(8.0)
                        .child(
                            VStack::new()
                                .child(
                                    MenuItem::new_literal("Normal item")
                                        .on_activate_fn(|_| println!("Cut"))
                                        .shortcut_label("Ctrl+X"),
                                )
                                .child(
                                    MenuItem::new_literal("With icon")
                                        .on_activate_fn(|_| println!("Copy"))
                                        .icon(IconWidget::checkmark(16.0))
                                        .shortcut_label("Ctrl+C"),
                                )
                                .child(MenuItem::new_literal("Disabled item").enabled(false))
                                .child(MenuItem::submenu_literal("Submenu trigger", || {
                                    Box::new(TextWidget::new_literal("submenu placeholder"))
                                })),
                        ),
                ),
        );

        // --- Assemble ---

        let toolbar = ctx.add(
            Toolbar::new().child(
                HStack::new()
                    .child(
                        TextWidget::new_literal("Menus & Dropdowns")
                            .style(t.body_bold.clone())
                            .color(TextRole::Primary),
                    )
                    .child(Spacer::new())
                    .child(
                        Button::new_literal("Toggle Dark Mode")
                            .style(ButtonVariant::Regular)
                            .on_activate_fn(|_| println!("ToggleDarkMode")),
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
        let padded = ctx.add(Padding::uniform(24.0).child_id(content));
        let scroll = ctx.add(
            ScrollArea::from_id(padded).scroll_bar_style(fern_ui::widgets::ScrollBarMode::Overlay),
        );

        let menu_bar = ctx.add(
            MenuBar::new()
                .leading_slot(IconWidget::chevron_right(16.0).color(TextRole::Accent))
                .menu_literal("File", || {
                    Box::new(
                        MenuList::new()
                            .item(
                                MenuItem::new_literal("New")
                                    .on_activate_fn(|_| println!("NewFile"))
                                    .shortcut_label("Ctrl+N"),
                            )
                            .item(
                                MenuItem::new_literal("Open")
                                    .on_activate_fn(|_| println!("OpenFile"))
                                    .shortcut_label("Ctrl+O"),
                            )
                            .item(
                                MenuItem::new_literal("Save")
                                    .on_activate_fn(|_| println!("SaveFile"))
                                    .shortcut_label("Ctrl+S"),
                            )
                            .separator()
                            .item(MenuItem::new_literal("Quit").on_activate_fn(|_| println!("ToggleDarkMode"))),
                    )
                })
                .menu_literal("Edit", || {
                    Box::new(
                        MenuList::new()
                            .item(
                                MenuItem::new_literal("Undo")
                                    .on_activate_fn(|_| println!("Undo"))
                                    .shortcut_label("Ctrl+Z"),
                            )
                            .item(
                                MenuItem::new_literal("Redo")
                                    .on_activate_fn(|_| println!("Redo"))
                                    .shortcut_label("Ctrl+Shift+Z"),
                            )
                            .separator()
                            .item(
                                MenuItem::new_literal("Cut")
                                    .on_activate_fn(|_| println!("Cut"))
                                    .shortcut_label("Ctrl+X"),
                            )
                            .item(
                                MenuItem::new_literal("Copy")
                                    .on_activate_fn(|_| println!("Copy"))
                                    .shortcut_label("Ctrl+C"),
                            )
                            .item(
                                MenuItem::new_literal("Paste")
                                    .on_activate_fn(|_| println!("Paste"))
                                    .shortcut_label("Ctrl+V"),
                            )
                            .separator()
                            .item(
                                MenuItem::new_literal("Select All")
                                    .on_activate_fn(|_| println!("SelectAll"))
                                    .shortcut_label("Ctrl+A"),
                            ),
                    )
                })
                .menu_literal("View", || {
                    Box::new(
                        MenuList::new()
                            .item(MenuItem::submenu_literal("Alignment", || {
                                Box::new(
                                    MenuList::new()
                                        .item(MenuItem::new_literal("Left").on_activate_fn(|_| println!("AlignLeft")))
                                        .item(MenuItem::new_literal("Center").on_activate_fn(|_| println!("AlignCenter")))
                                        .item(MenuItem::new_literal("Right").on_activate_fn(|_| println!("AlignRight")))
                                        .item(
                                            MenuItem::new_literal("Justify").on_activate_fn(|_| println!("AlignJustify")),
                                        ),
                                )
                            }))
                            .separator()
                            .item(
                                MenuItem::new_literal("Toggle Dark Mode").on_activate_fn(|_| println!("ToggleDarkMode")),
                            ),
                    )
                })
                .trailing_slot(
                    Button::new_literal("Settings")
                        .style(ButtonVariant::Flat)
                        .on_activate_fn(|_| println!("ToggleDarkMode")),
                ),
        );

        let root = ctx.add(
            VStack::new()
                .add_child(menu_bar)
                .add_child(toolbar)
                .child(Expand::new().fills_stack().child_id(scroll))
                .child(
                    StatusBar::new().child(
                        TextWidget::new_literal("Milestone 4 -- Menus & Dropdowns")
                            .style(t.tiny.clone())
                            .color(TextRole::Primary),
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
    FernAppBuilder::new()
        .theme(Theme::light_default())
        .window_title("FernUI — Menus & Dropdowns (Milestone 4)")
        .window_size(900, 700)
        .root(|tree| tree.add(Root::new()))
        .run();
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------
