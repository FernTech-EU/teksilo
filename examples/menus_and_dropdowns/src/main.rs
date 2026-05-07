//! Milestone 4: Menus & Dropdowns
//!
//! Demonstrates the overlay-based interactive widgets added in Milestone 4:
//! - ComboBox with dropdown selection
//! - Context menu (right-click) with MenuList and MenuItem
//! - MenuItem with icons, shortcut labels, disabled items, separators
//! - Theme switching via context menu command
//!
//! Run with: `cargo run -p menus-and-dropdowns`

use fern_ui::IntentKind;
use fern_ui::core::Action;
use fern_ui::prelude::*;
use fern_ui::widgets::{
    Button, ButtonVariant, ComboBox, Divider, Expand, HStack, IconButton, IconWidget, MenuBar,
    MenuItem, MenuList, Padding, Panel, PopoverButton, PopoverIconButton, ScrollArea, Slider,
    Spacer, StatusBar, TextWidget, Toolbar, VStack,
};

#[derive(Debug, IntentKind)]
enum AppIntent {
    #[name = "menus.toggle_dark_mode"]
    ToggleDarkMode,
}

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
        let _theme = ctx.theme_signal().get();

        let is_dark = ctx.signal(false);
        let is_dark_for_action = is_dark.clone();
        ctx.register_action(Action::new("menus.toggle_dark_mode").on_invoke(
            move |_intent, ctx| {
                let dark = !is_dark_for_action.get();
                is_dark_for_action.set(dark);
                ctx.set_theme(if dark {
                    Theme::dark_default()
                } else {
                    Theme::light_default()
                });
            },
        ));

        // --- Section 1: ComboBox demos ---

        let fruit_selected = ctx.signal(None::<String>);
        let color_selected = ctx.signal(Some("Blue".to_string())); // pre-select "Blue"
        let size_selected = ctx.signal(None::<String>);
        // Searchable combo — its query signal is held externally so the
        // caller could observe or clear it. Here we just let it live
        // alongside the selection signal.
        let country_selected = ctx.signal(None::<String>);
        let country_query = ctx.signal(String::new());
        // Huge combo — 10 000 items to exercise the ListView-backed
        // virtualization path. Opening this would be prohibitively slow
        // without virtualization; with it, only the ~visible rows (plus
        // a small buffer) are materialized.
        let huge_selected = ctx.signal(None::<String>);
        let huge_items: Vec<String> = (0..10_000).map(|i| format!("Item #{i:05}")).collect();

        let combo_section = ctx.add(
            VStack::new()
                .spacing(12.0)
                .child(
                    TextWidget::new_literal("ComboBox / Dropdown")
                        .style(TextStyleRole::BodyBold)
                        .color(TextRole::Primary),
                )
                .child(
                    TextWidget::new_literal(
                        "Click to open the dropdown. Use arrow keys to navigate, \
                         Enter to select, Escape to close. The searchable variant \
                         adds a text field at the top of the panel that filters the \
                         list live.",
                    )
                    .style(TextStyleRole::Body)
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
                                        .style(TextStyleRole::Small)
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
                                        .style(TextStyleRole::Small)
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
                                        .style(TextStyleRole::Small)
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
                                        .style(TextStyleRole::Small)
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
                        )
                        .child(
                            VStack::new()
                                .spacing(4.0)
                                .child(
                                    TextWidget::new_literal("Huge (10 000 items)")
                                        .style(TextStyleRole::Small)
                                        .color(TextRole::Primary),
                                )
                                .child(
                                    ComboBox::new(huge_items, huge_selected.clone())
                                        .placeholder("Open me — virtualized")
                                        .max_visible_items(10),
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
                        .style(TextStyleRole::BodyBold)
                        .color(TextRole::Primary),
                )
                .child(
                    TextWidget::new_literal(
                        "Right-click on the panels below to open a context menu. \
                         Each panel has a different menu.",
                    )
                    .style(TextStyleRole::Body)
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
                                                .style(TextStyleRole::Small)
                                                .color(TextRole::Primary),
                                        )
                                        .child(
                                            TextWidget::new_literal(
                                                "Right-click for Cut/Copy/Paste",
                                            )
                                            .style(TextStyleRole::Body)
                                            .color(TextRole::Primary),
                                        ),
                                )
                                .context_menu(|_pos, _ctx| {
                                    Some(Box::new(
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
                                                                .on_activate_fn(|_| {
                                                                    println!("AlignLeft")
                                                                }),
                                                        )
                                                        .item(
                                                            MenuItem::new_literal("Center")
                                                                .on_activate_fn(|_| {
                                                                    println!("AlignCenter")
                                                                }),
                                                        )
                                                        .item(
                                                            MenuItem::new_literal("Right")
                                                                .on_activate_fn(|_| {
                                                                    println!("AlignRight")
                                                                }),
                                                        )
                                                        .item(
                                                            MenuItem::new_literal("Justify")
                                                                .on_activate_fn(|_| {
                                                                    println!("AlignJustify")
                                                                }),
                                                        ),
                                                )
                                            })),
                                    ))
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
                                                .style(TextStyleRole::Small)
                                                .color(TextRole::Primary),
                                        )
                                        .child(
                                            TextWidget::new_literal(
                                                "Right-click for file operations",
                                            )
                                            .style(TextStyleRole::Body)
                                            .color(TextRole::Primary),
                                        ),
                                )
                                .context_menu(|_pos, _ctx| {
                                    Some(Box::new(
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
                                            .item(
                                                MenuItem::new_literal("Export as PDF")
                                                    .enabled(false),
                                            ),
                                    ))
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
                        .style(TextStyleRole::BodyBold)
                        .color(TextRole::Primary),
                )
                .child(
                    TextWidget::new_literal(
                        "MenuItems shown directly (not in an overlay) to demonstrate \
                         their visual styles and interaction states.",
                    )
                    .style(TextStyleRole::Body)
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

        // --- Section 4: Rich-content menu ---
        //
        // `MenuList::item(...)` accepts any `impl Widget + 'static`, so a
        // menu can mix plain `MenuItem` rows with arbitrary controls —
        // a quick-action button column, a slider, a combo box, a regular
        // button. Mouse interaction works on every child; arrow keys
        // highlight rows but only `MenuItem`s activate via Enter.

        let opacity_signal = ctx.signal(0.65_f32);
        let theme_choice = ctx.signal(Some("Auto".to_string()));
        let pinned = ctx.signal(false);

        let rich_menu_section = ctx.add(
            VStack::new()
                .spacing(12.0)
                .child(
                    TextWidget::new_literal("Rich-content menu")
                        .style(TextStyleRole::BodyBold)
                        .color(TextRole::Primary),
                )
                .child(
                    TextWidget::new_literal(
                        "Click the button below to open a menu mixing a column of icon \
                         actions, a slider, a combo box, a regular button, and plain \
                         menu items. `MenuList` accepts any widget via `.item(...)`.",
                    )
                    .style(TextStyleRole::Body)
                    .color(TextRole::Primary),
                )
                .child(
                    TextWidget::new_literal(
                        "Or use a PopoverIconButton — same overlay wiring, square \
                         icon-only trigger with a disclosure caret painted in the \
                         bottom-right corner.",
                    )
                    .style(TextStyleRole::Small)
                    .color(TextRole::Secondary),
                )
                .child(
                    HStack::new()
                        .spacing(12.0)
                        .child(
                            PopoverIconButton::new(IconButton::add().toolbar()).content(
                                MenuList::new()
                                    .item(
                                        MenuItem::new_literal("New file")
                                            .on_activate_fn(|_| println!("NewFileFromPopoverIcon")),
                                    )
                                    .item(
                                        MenuItem::new_literal("New folder").on_activate_fn(|_| {
                                            println!("NewFolderFromPopoverIcon")
                                        }),
                                    )
                                    .separator()
                                    .item(
                                        MenuItem::new_literal("New project…")
                                            .on_activate_fn(|_| println!("NewProject")),
                                    ),
                            ),
                        )
                        .child(
                            PopoverIconButton::new(IconButton::search().large()).content(
                                MenuList::new()
                                    .item(
                                        MenuItem::new_literal("Search files")
                                            .shortcut_label("Ctrl+P")
                                            .on_activate_fn(|_| println!("SearchFiles")),
                                    )
                                    .item(
                                        MenuItem::new_literal("Search symbols")
                                            .shortcut_label("Ctrl+T")
                                            .on_activate_fn(|_| println!("SearchSymbols")),
                                    )
                                    .item(
                                        MenuItem::new_literal("Search everywhere")
                                            .shortcut_label("Shift+Shift")
                                            .on_activate_fn(|_| println!("SearchEverywhere")),
                                    ),
                            ),
                        ),
                )
                .child(
                    PopoverButton::new(
                        Button::new_literal("View options").style(ButtonVariant::Regular),
                    )
                    .show_disclosure_caret(true)
                    .content(
                        MenuList::new()
                            // Row of square, icon-only, flat IconButtons at
                            // Toolbar (40 dp) size — stand-alone visual mode
                            // (full-weight icons). The trailing one is bistate
                            // via `.toggle(pinned)` — clicking flips the
                            // signal and the surface reads as Selected while
                            // `pinned == true`.
                            .item(
                                Padding::symmetric(6.0, 6.0).child(
                                    HStack::new()
                                        .spacing(4.0)
                                        .child(
                                            IconButton::new(IconWidget::chevron_left(20.0))
                                                .toolbar()
                                                .tooltip_literal("Previous")
                                                .on_activate_fn(|_| println!("Prev")),
                                        )
                                        .child(
                                            IconButton::new(IconWidget::chevron_right(20.0))
                                                .toolbar()
                                                .tooltip_literal("Next")
                                                .on_activate_fn(|_| println!("Next")),
                                        )
                                        .child(
                                            IconButton::new(IconWidget::chevron_up(20.0))
                                                .toolbar()
                                                .tooltip_literal("Move up")
                                                .on_activate_fn(|_| println!("MoveUp")),
                                        )
                                        .child(
                                            IconButton::new(IconWidget::chevron_down(20.0))
                                                .toolbar()
                                                .tooltip_literal("Move down")
                                                .on_activate_fn(|_| println!("MoveDown")),
                                        )
                                        .child(
                                            IconButton::new(IconWidget::checkmark(20.0))
                                                .toolbar()
                                                .tooltip_literal("Pin (bistate)")
                                                .toggle(pinned.clone())
                                                .on_activate_fn(|_| println!("TogglePin")),
                                        ),
                                ),
                            )
                            .separator()
                            // Column of embedded-mode IconButtons — the
                            // dim-at-rest "built-in" look, useful for
                            // secondary quick actions tucked into a menu.
                            .item(
                                Padding::symmetric(6.0, 6.0).child(
                                    VStack::new()
                                        .spacing(2.0)
                                        .child(
                                            IconButton::copy()
                                                .embedded()
                                                .on_activate_fn(|_| println!("QuickCopy")),
                                        )
                                        .child(
                                            IconButton::clear()
                                                .embedded()
                                                .on_activate_fn(|_| println!("QuickClear")),
                                        )
                                        .child(
                                            IconButton::add()
                                                .embedded()
                                                .on_activate_fn(|_| println!("QuickAdd")),
                                        )
                                        .child(
                                            IconButton::search()
                                                .embedded()
                                                .on_activate_fn(|_| println!("QuickSearch")),
                                        ),
                                ),
                            )
                            .separator()
                            // Labelled slider.
                            .item(
                                Padding::symmetric(6.0, 10.0).child(
                                    VStack::new()
                                        .spacing(4.0)
                                        .child(
                                            TextWidget::new_literal("Opacity")
                                                .style(TextStyleRole::Small)
                                                .color(TextRole::Primary),
                                        )
                                        .child(
                                            Slider::new(opacity_signal.clone(), 0.0, 1.0)
                                                .label_literal("Opacity"),
                                        ),
                                ),
                            )
                            // Labelled combo box.
                            .item(
                                Padding::symmetric(6.0, 10.0).child(
                                    VStack::new()
                                        .spacing(4.0)
                                        .child(
                                            TextWidget::new_literal("Theme")
                                                .style(TextStyleRole::Small)
                                                .color(TextRole::Primary),
                                        )
                                        .child(ComboBox::new(
                                            vec!["Auto", "Light", "Dark", "High Contrast"],
                                            theme_choice.clone(),
                                        )),
                                ),
                            )
                            // Plain button.
                            .item(
                                Padding::symmetric(6.0, 10.0).child(
                                    Button::new_literal("Advanced settings…")
                                        .style(ButtonVariant::Flat)
                                        .on_activate_fn(|_| println!("AdvancedSettings")),
                                ),
                            )
                            .separator()
                            // Plain menu items still work alongside rich content.
                            .item(
                                MenuItem::new_literal("Reset to defaults")
                                    .on_activate_fn(|_| println!("ResetDefaults")),
                            )
                            .item(
                                MenuItem::new_literal("Close menu")
                                    .shortcut_label("Esc")
                                    .on_activate_fn(|_| println!("CloseMenu")),
                            ),
                    ),
                ),
        );

        // --- Assemble ---

        let toolbar = ctx.add(
            Toolbar::new().child(
                HStack::new()
                    .child(
                        TextWidget::new_literal("Menus & Dropdowns")
                            .style(TextStyleRole::BodyBold)
                            .color(TextRole::Primary),
                    )
                    .child(Spacer::new())
                    .child(
                        Button::new_literal("Toggle Dark Mode")
                            .style(ButtonVariant::Regular)
                            .on_activate_fn(|ctx| {
                                ctx.send_intent(AppIntent::ToggleDarkMode);
                            }),
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
                .add_child(menu_showcase_section)
                .child(Divider::new().thickness(2.0))
                .add_child(rich_menu_section),
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
                            .item(
                                MenuItem::new_literal("Quit").on_activate_fn(|_| println!("Quit")),
                            ),
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
                            }))
                            .separator()
                            .item(MenuItem::new_literal("Toggle Dark Mode").on_activate_fn(
                                |ctx| {
                                    ctx.send_intent(AppIntent::ToggleDarkMode);
                                },
                            )),
                    )
                })
                .trailing_slot(
                    Button::new_literal("Settings")
                        .style(ButtonVariant::Flat)
                        .on_activate_fn(|_| println!("Settings")),
                ),
        );

        let root = ctx.add(
            VStack::new()
                .add_child(menu_bar)
                .add_child(toolbar)
                .child(Expand::new().child_id(scroll))
                .child(
                    StatusBar::new().child(
                        TextWidget::new_literal("Milestone 4 -- Menus & Dropdowns")
                            .style(TextStyleRole::Tiny)
                            .color(TextRole::Primary),
                    ),
                ),
        );

        self.root_child_id = Some(root);
        vec![root]
    }

    fn layout_response(&self, proposal: SizeProposal, ctx: &LayoutContext) -> LayoutResponse {
        match self.root_child_id {
            Some(id) => ctx
                .child_size(id, proposal)
                .unwrap_or_else(|| proposal.resolve(0.0, 0.0)),
            None => proposal.resolve(0.0, 0.0),
        }
        .into()
    }
}

// ---------------------------------------------------------------------------
// Entry point
// ---------------------------------------------------------------------------

fn main() {
    FernAppBuilder::new()
        .install_inspector_in_debug()
        .theme(Theme::light_default())
        .initial_window(
            WindowConfig::new()
                .title("FernUI — Menus & Dropdowns (Milestone 4)")
                .size(900, 700)
                .root(|tree, _state| tree.add(Root::new())),
        )
        .run();
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------
