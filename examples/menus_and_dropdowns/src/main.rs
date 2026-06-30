// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Menus & Dropdowns
//!
//! Demonstrates the menu system:
//! - ComboBox with dropdown selection
//! - Context menu (right-click) with MenuList and MenuItem
//! - MenuBar with `&`-marker mnemonics (hold Alt to see the underlines;
//!   Alt+F / Alt+E / Alt+V opens the matching menu; F10 focuses the
//!   menubar; bare Alt-tap focuses the menubar)
//! - MenuItem in all three modes: plain, checkable (two-state and
//!   tri-state), and radio
//! - In-menu mnemonic activation (open a menu, press the underlined letter)
//! - Type-ahead inside menus (open the View menu, type a few letters)
//! - Submenu hover + safe-triangle (open File → Recent and sweep
//!   diagonally toward the submenu)
//!
//! Run with: `cargo run -p menus-and-dropdowns`

use bastyde::prelude::*;
use bastyde::widgets::{
    Button, ButtonVariant, ComboBox, Divider, Expand, HStack, IconButton, IconWidget, MenuBar,
    MenuItem, MenuList, Padding, Panel, PopoverButton, PopoverIconButton, ScrollArea, Slider,
    Spacer, StatusBar, TextWidget, Toolbar, VStack,
};
use bastyde_data::CheckState;

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

        // State for the View-menu showcase. All three patterns share the
        // same MenuList-built `Signal`s so MenuList's auto-grouping
        // (`Signal::same`) bundles the three radio items.
        let word_wrap = ctx.signal(true);
        let inspector_state = ctx.signal(CheckState::Indeterminate);
        // 0 = Light, 1 = Dark, 2 = System. Named `theme_radio_choice`
        // (not `theme_choice`) to avoid shadowing the existing
        // `Signal<Option<String>>` further down used by the
        // SettingsPanel-style ComboBox demo.
        let theme_radio_choice = ctx.signal(0_usize);

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
                    TextWidget::new(lit!("ComboBox / Dropdown"))
                        .style(TextStyleRole::BodyBold)
                        .color(TextRole::Primary),
                )
                .child(
                    TextWidget::new(lit!(
                        "Click to open the dropdown. Use arrow keys to navigate, \
                         Enter to select, Escape to close. The searchable variant \
                         adds a text field at the top of the panel that filters the \
                         list live."
                    ))
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
                                    TextWidget::new(lit!("Fruit"))
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
                                    .placeholder(lit!("Select a fruit...")),
                                ),
                        )
                        .child(
                            VStack::new()
                                .spacing(4.0)
                                .child(
                                    TextWidget::new(lit!("Color"))
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
                                    TextWidget::new(lit!("Size (disabled)"))
                                        .style(TextStyleRole::Small)
                                        .color(TextRole::Primary),
                                )
                                .child(
                                    ComboBox::new(
                                        vec!["Small", "Medium", "Large"],
                                        size_selected.clone(),
                                    )
                                    .placeholder(lit!("Choose size"))
                                    .enabled(false),
                                ),
                        )
                        .child(
                            VStack::new()
                                .spacing(4.0)
                                .child(
                                    TextWidget::new(lit!("Country (searchable)"))
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
                                    .placeholder(lit!("Pick a country…"))
                                    .search_query(country_query.clone())
                                    .max_visible_items(6),
                                ),
                        )
                        .child(
                            VStack::new()
                                .spacing(4.0)
                                .child(
                                    TextWidget::new(lit!("Huge (10 000 items)"))
                                        .style(TextStyleRole::Small)
                                        .color(TextRole::Primary),
                                )
                                .child(
                                    ComboBox::new(huge_items, huge_selected.clone())
                                        .placeholder(lit!("Open me — virtualized"))
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
                    TextWidget::new(lit!("Context Menu"))
                        .style(TextStyleRole::BodyBold)
                        .color(TextRole::Primary),
                )
                .child(
                    TextWidget::new(lit!(
                        "Right-click on the panels below to open a context menu. \
                         Each panel has a different menu."
                    ))
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
                                            TextWidget::new(lit!("Edit Menu"))
                                                .style(TextStyleRole::Small)
                                                .color(TextRole::Primary),
                                        )
                                        .child(
                                            TextWidget::new(lit!("Right-click for Cut/Copy/Paste"))
                                                .style(TextStyleRole::Body)
                                                .color(TextRole::Primary),
                                        ),
                                )
                                .context_menu(|_pos, _ctx| {
                                    Some(Box::new(
                                        MenuList::new()
                                            .item(
                                                MenuItem::new(lit!("Undo"))
                                                    .on_activate_fn(|_| println!("Undo"))
                                                    .shortcut_label("Ctrl+Z"),
                                            )
                                            .item(
                                                MenuItem::new(lit!("Redo"))
                                                    .on_activate_fn(|_| println!("Redo"))
                                                    .shortcut_label("Ctrl+Shift+Z"),
                                            )
                                            .separator()
                                            .item(
                                                MenuItem::new(lit!("Cut"))
                                                    .on_activate_fn(|_| println!("Cut"))
                                                    .shortcut_label("Ctrl+X"),
                                            )
                                            .item(
                                                MenuItem::new(lit!("Copy"))
                                                    .on_activate_fn(|_| println!("Copy"))
                                                    .shortcut_label("Ctrl+C"),
                                            )
                                            .item(
                                                MenuItem::new(lit!("Paste"))
                                                    .on_activate_fn(|_| println!("Paste"))
                                                    .shortcut_label("Ctrl+V"),
                                            )
                                            .separator()
                                            .item(
                                                MenuItem::new(lit!("Select All"))
                                                    .on_activate_fn(|_| println!("SelectAll"))
                                                    .shortcut_label("Ctrl+A"),
                                            )
                                            .separator()
                                            .item(MenuItem::submenu(lit!("Alignment"), || {
                                                Box::new(
                                                    MenuList::new()
                                                        .item(
                                                            MenuItem::new(lit!("Left"))
                                                                .on_activate_fn(|_| {
                                                                    println!("AlignLeft")
                                                                }),
                                                        )
                                                        .item(
                                                            MenuItem::new(lit!("Center"))
                                                                .on_activate_fn(|_| {
                                                                    println!("AlignCenter")
                                                                }),
                                                        )
                                                        .item(
                                                            MenuItem::new(lit!("Right"))
                                                                .on_activate_fn(|_| {
                                                                    println!("AlignRight")
                                                                }),
                                                        )
                                                        .item(
                                                            MenuItem::new(lit!("Justify"))
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
                                            TextWidget::new(lit!("File Menu"))
                                                .style(TextStyleRole::Small)
                                                .color(TextRole::Primary),
                                        )
                                        .child(
                                            TextWidget::new(lit!(
                                                "Right-click for file operations"
                                            ))
                                            .style(TextStyleRole::Body)
                                            .color(TextRole::Primary),
                                        ),
                                )
                                .context_menu(|_pos, _ctx| {
                                    Some(Box::new(
                                        MenuList::new()
                                            .item(
                                                MenuItem::new(lit!("New File"))
                                                    .on_activate_fn(|_| println!("NewFile"))
                                                    .shortcut_label("Ctrl+N"),
                                            )
                                            .item(
                                                MenuItem::new(lit!("Open File..."))
                                                    .on_activate_fn(|_| println!("OpenFile"))
                                                    .shortcut_label("Ctrl+O"),
                                            )
                                            .item(
                                                MenuItem::new(lit!("Save"))
                                                    .on_activate_fn(|_| println!("SaveFile"))
                                                    .shortcut_label("Ctrl+S"),
                                            )
                                            .separator()
                                            .item(
                                                MenuItem::new(lit!("Export as PDF")).enabled(false),
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
                    TextWidget::new(lit!("Menu Items (inline)"))
                        .style(TextStyleRole::BodyBold)
                        .color(TextRole::Primary),
                )
                .child(
                    TextWidget::new(lit!(
                        "MenuItems shown directly (not in an overlay) to demonstrate \
                         their visual styles and interaction states."
                    ))
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
                                    MenuItem::new(lit!("Normal item"))
                                        .on_activate_fn(|_| println!("Cut"))
                                        .shortcut_label("Ctrl+X"),
                                )
                                .child(
                                    MenuItem::new(lit!("With icon"))
                                        .on_activate_fn(|_| println!("Copy"))
                                        .icon(IconWidget::checkmark(16.0))
                                        .shortcut_label("Ctrl+C"),
                                )
                                .child(MenuItem::new(lit!("Disabled item")).enabled(false))
                                .child(MenuItem::submenu(lit!("Submenu trigger"), || {
                                    Box::new(TextWidget::new(lit!("submenu placeholder")))
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
                    TextWidget::new(lit!("Rich-content menu"))
                        .style(TextStyleRole::BodyBold)
                        .color(TextRole::Primary),
                )
                .child(
                    TextWidget::new(lit!(
                        "Click the button below to open a menu mixing a column of icon \
                         actions, a slider, a combo box, a regular button, and plain \
                         menu items. `MenuList` accepts any widget via `.item(...)`."
                    ))
                    .style(TextStyleRole::Body)
                    .color(TextRole::Primary),
                )
                .child(
                    TextWidget::new(lit!(
                        "Or use a PopoverIconButton — same overlay wiring, square \
                         icon-only trigger with a disclosure caret painted in the \
                         bottom-right corner."
                    ))
                    .style(TextStyleRole::Small)
                    .color(TextRole::Secondary),
                )
                .child(
                    HStack::new()
                        .spacing(12.0)
                        .child(
                            PopoverIconButton::new(IconButton::add().toolbar())
                                .bare()
                                .content(
                                    MenuList::new()
                                        .item(
                                            MenuItem::new(lit!("New file")).on_activate_fn(|_| {
                                                println!("NewFileFromPopoverIcon")
                                            }),
                                        )
                                        .item(MenuItem::new(lit!("New folder")).on_activate_fn(
                                            |_| println!("NewFolderFromPopoverIcon"),
                                        ))
                                        .separator()
                                        .item(
                                            MenuItem::new(lit!("New project…"))
                                                .on_activate_fn(|_| println!("NewProject")),
                                        ),
                                ),
                        )
                        .child(
                            PopoverIconButton::new(IconButton::search().large())
                                .bare()
                                .content(
                                    MenuList::new()
                                        .item(
                                            MenuItem::new(lit!("Search files"))
                                                .shortcut_label("Ctrl+P")
                                                .on_activate_fn(|_| println!("SearchFiles")),
                                        )
                                        .item(
                                            MenuItem::new(lit!("Search symbols"))
                                                .shortcut_label("Ctrl+T")
                                                .on_activate_fn(|_| println!("SearchSymbols")),
                                        )
                                        .item(
                                            MenuItem::new(lit!("Search everywhere"))
                                                .shortcut_label("Shift+Shift")
                                                .on_activate_fn(|_| println!("SearchEverywhere")),
                                        ),
                                ),
                        ),
                )
                .child(
                    PopoverButton::new(
                        Button::new(lit!("View options")).variant(ButtonVariant::Plain),
                    )
                    .show_disclosure_caret(true)
                    .bare()
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
                                                .tooltip(lit!("Previous"))
                                                .on_activate_fn(|_| println!("Prev")),
                                        )
                                        .child(
                                            IconButton::new(IconWidget::chevron_right(20.0))
                                                .toolbar()
                                                .tooltip(lit!("Next"))
                                                .on_activate_fn(|_| println!("Next")),
                                        )
                                        .child(
                                            IconButton::new(IconWidget::chevron_up(20.0))
                                                .toolbar()
                                                .tooltip(lit!("Move up"))
                                                .on_activate_fn(|_| println!("MoveUp")),
                                        )
                                        .child(
                                            IconButton::new(IconWidget::chevron_down(20.0))
                                                .toolbar()
                                                .tooltip(lit!("Move down"))
                                                .on_activate_fn(|_| println!("MoveDown")),
                                        )
                                        .child(
                                            IconButton::new(IconWidget::checkmark(20.0))
                                                .toolbar()
                                                .tooltip(lit!("Pin (bistate)"))
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
                                            TextWidget::new(lit!("Opacity"))
                                                .style(TextStyleRole::Small)
                                                .color(TextRole::Primary),
                                        )
                                        .child(
                                            Slider::new(opacity_signal.clone(), 0.0, 1.0)
                                                .label(lit!("Opacity")),
                                        ),
                                ),
                            )
                            // Labelled combo box.
                            .item(
                                Padding::symmetric(6.0, 10.0).child(
                                    VStack::new()
                                        .spacing(4.0)
                                        .child(
                                            TextWidget::new(lit!("Theme"))
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
                                    Button::new(lit!("Advanced settings…"))
                                        .variant(ButtonVariant::Ghost)
                                        .on_activate_fn(|_| println!("AdvancedSettings")),
                                ),
                            )
                            .separator()
                            // Plain menu items still work alongside rich content.
                            .item(
                                MenuItem::new(lit!("Reset to defaults"))
                                    .on_activate_fn(|_| println!("ResetDefaults")),
                            )
                            .item(
                                MenuItem::new(lit!("Close menu"))
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
                        TextWidget::new(lit!("Menus & Dropdowns"))
                            .style(TextStyleRole::BodyBold)
                            .color(TextRole::Primary),
                    )
                    .child(Spacer::new())
                    .child(bastyde::widgets::ThemeSwitcher::new()),
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
            ScrollArea::from_id(padded).scroll_bar_style(bastyde::widgets::ScrollBarMode::Overlay),
        );

        let word_wrap_for_view = word_wrap.clone();
        let inspector_for_view = inspector_state.clone();
        let theme_choice_for_view = theme_radio_choice.clone();

        let menu_bar = ctx.add(
            MenuBar::new()
                .leading_slot(IconWidget::chevron_right(16.0).color(TextRole::Accent))
                // `&File` parses to "File" with 'F' marked as the
                // mnemonic. Hold Alt to see the underline; Alt+F opens
                // this menu. Inside the menu, bare 'N' / 'O' / 'S' / 'Q'
                // activates the matching item.
                .menu(lit!("&File"), || {
                    Box::new(
                        MenuList::new()
                            .item(
                                MenuItem::new(lit!("&New"))
                                    .on_activate_fn(|_| println!("NewFile"))
                                    .shortcut_label("Ctrl+N"),
                            )
                            .item(
                                MenuItem::new(lit!("&Open"))
                                    .on_activate_fn(|_| println!("OpenFile"))
                                    .shortcut_label("Ctrl+O"),
                            )
                            .item(
                                MenuItem::new(lit!("&Save"))
                                    .on_activate_fn(|_| println!("SaveFile"))
                                    .shortcut_label("Ctrl+S"),
                            )
                            // A submenu trigger. Hovering over "Recent"
                            // opens the submenu after the 400ms delay;
                            // sweeping diagonally toward the submenu
                            // doesn't collapse it thanks to the
                            // safe-triangle hover gate.
                            .item(MenuItem::submenu(lit!("&Recent"), || {
                                Box::new(
                                    MenuList::new()
                                        .item(
                                            MenuItem::new(lit!("project-alpha.toml"))
                                                .on_activate_fn(|_| println!("Recent: alpha")),
                                        )
                                        .item(
                                            MenuItem::new(lit!("notes.md"))
                                                .on_activate_fn(|_| println!("Recent: notes")),
                                        )
                                        .item(
                                            MenuItem::new(lit!("budget.csv"))
                                                .on_activate_fn(|_| println!("Recent: budget")),
                                        ),
                                )
                            }))
                            .separator()
                            .item(
                                MenuItem::new(lit!("&Quit")).on_activate_fn(|_| println!("Quit")),
                            ),
                    )
                })
                .menu(lit!("&Edit"), || {
                    Box::new(
                        MenuList::new()
                            .item(
                                MenuItem::new(lit!("&Undo"))
                                    .on_activate_fn(|_| println!("Undo"))
                                    .shortcut_label("Ctrl+Z"),
                            )
                            .item(
                                MenuItem::new(lit!("&Redo"))
                                    .on_activate_fn(|_| println!("Redo"))
                                    .shortcut_label("Ctrl+Shift+Z"),
                            )
                            .separator()
                            .item(
                                MenuItem::new(lit!("Cu&t"))
                                    .on_activate_fn(|_| println!("Cut"))
                                    .shortcut_label("Ctrl+X"),
                            )
                            .item(
                                MenuItem::new(lit!("&Copy"))
                                    .on_activate_fn(|_| println!("Copy"))
                                    .shortcut_label("Ctrl+C"),
                            )
                            .item(
                                MenuItem::new(lit!("&Paste"))
                                    .on_activate_fn(|_| println!("Paste"))
                                    .shortcut_label("Ctrl+V"),
                            )
                            .separator()
                            .item(
                                MenuItem::new(lit!("Select &All"))
                                    .on_activate_fn(|_| println!("SelectAll"))
                                    .shortcut_label("Ctrl+A"),
                            ),
                    )
                })
                .menu(lit!("&View"), move || {
                    Box::new(
                        MenuList::new()
                            // CHECKABLE — two-state. The checkmark
                            // glyph in the leading slot follows
                            // `word_wrap`.
                            .item(
                                MenuItem::new(lit!("&Word Wrap"))
                                    .bind_checked(word_wrap_for_view.clone()),
                            )
                            // CHECKABLE — tri-state. Cycles
                            // Unchecked↔Checked on click; Indeterminate
                            // is the externally-supplied initial state
                            // (shown as a dash glyph).
                            .item(
                                MenuItem::new(lit!("Show &Inspector"))
                                    .bind_check_state(inspector_for_view.clone()),
                            )
                            .separator()
                            // RADIO GROUP — three items sharing one
                            // `Signal<usize>`. MenuList groups them by
                            // signal identity and announces "2 of 3"
                            // via `push_to_radio_group`. Clicking writes the
                            // matching value into `theme_radio_choice` AND
                            // applies the theme (set_theme / follow_system_theme),
                            // staying in sync with the toolbar's ThemeSwitcher.
                            .item(
                                MenuItem::new(lit!("&Light Theme"))
                                    .radio(0, theme_choice_for_view.clone())
                                    .on_activate_fn(|ctx| {
                                        ctx.set_theme(bastyde::presets::intui::light())
                                    }),
                            )
                            .item(
                                MenuItem::new(lit!("&Dark Theme"))
                                    .radio(1, theme_choice_for_view.clone())
                                    .on_activate_fn(|ctx| {
                                        ctx.set_theme(bastyde::presets::intui::dark())
                                    }),
                            )
                            .item(
                                MenuItem::new(lit!("&System Theme"))
                                    .radio(2, theme_choice_for_view.clone())
                                    .on_activate_fn(|ctx| ctx.follow_system_theme()),
                            )
                            .separator()
                            .item(MenuItem::submenu(lit!("&Alignment"), || {
                                Box::new(
                                    MenuList::new()
                                        .item(
                                            MenuItem::new(lit!("&Left"))
                                                .on_activate_fn(|_| println!("AlignLeft")),
                                        )
                                        .item(
                                            MenuItem::new(lit!("&Center"))
                                                .on_activate_fn(|_| println!("AlignCenter")),
                                        )
                                        .item(
                                            MenuItem::new(lit!("&Right"))
                                                .on_activate_fn(|_| println!("AlignRight")),
                                        )
                                        .item(
                                            MenuItem::new(lit!("&Justify"))
                                                .on_activate_fn(|_| println!("AlignJustify")),
                                        ),
                                )
                            })),
                    )
                })
                .trailing_slot(
                    Button::new(lit!("Settings"))
                        .variant(ButtonVariant::Ghost)
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
                        TextWidget::new(lit!("Milestone 4 -- Menus & Dropdowns"))
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
    BastydeAppBuilder::new()
        .install_automation_bridge_in_debug()
        .install_inspector_in_debug()
        .theme(bastyde::presets::intui::light())
        .initial_window(
            WindowConfig::new()
                .title("Bastyde — Menus & Dropdowns (Milestone 4)")
                .size(900, 700)
                .root(|tree, _state| tree.add(Root::new())),
        )
        .run();
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------
