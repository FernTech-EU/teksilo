// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! What a screen reader is told while it works a menu.
//!
//! A reader in a menu hears two things: the menu, named after what opened it,
//! when focus enters it, and each item as the keyboard highlight reaches it.
//! Both are read here as a platform adapter reads them, through
//! `accesskit_consumer` and `common_filter` (see `common::heard_test`).

use std::time::Duration;

use accesskit_consumer::{NodeRef, Tree, common_filter};
use teksilo_canvas::{Point, SizeProposal};
use teksilo_core::accesskit::Role;
use teksilo_core::event::{Key, Modifiers};
use teksilo_core::widget_id::WidgetId;
use teksilo_core::widget_tree::WidgetTree;
use teksilo_i18n::lit;

use crate::common::heard_test::{Heard, Listener};
use crate::menu_item::MenuItem;
use crate::menu_list::MenuList;

fn light_tree() -> WidgetTree {
    WidgetTree::new().with_theme(teksilo_core::presets::intui::light())
}

fn lay_out(tree: &mut WidgetTree) {
    tree.layout(SizeProposal::exact(800.0, 600.0));
}

fn press(tree: &mut WidgetTree, key: Key) {
    tree.press_key(key, Modifiers::NONE);
    lay_out(tree);
}

fn focus(name: &str) -> Vec<Heard> {
    vec![Heard::Focus(name.to_string())]
}

/// A menu of four commands and a separator, holding keyboard focus with
/// nothing highlighted yet, and a reader listening from that moment.
fn focused_file_menu() -> (WidgetTree, WidgetId, Listener) {
    let mut tree = light_tree();
    let menu = tree.add(
        MenuList::new()
            .type_ahead_timeout(Duration::ZERO)
            .item(MenuItem::new(lit!("New")))
            .item(MenuItem::new(lit!("Open")))
            .separator()
            .item(MenuItem::new(lit!("Save")))
            .item(MenuItem::new(lit!("Quit"))),
    );
    lay_out(&mut tree);
    tree.focus(menu);
    lay_out(&mut tree);
    let listener = Listener::attach(&mut tree);
    (tree, menu, listener)
}

#[test]
fn every_highlight_move_puts_the_item_in_front_of_the_reader() {
    // The highlight used to be a private index: the menu kept focus, named
    // nothing as its active descendant, and every arrow press reached the
    // platform as nothing at all. Orca said "menu." on opening and was silent
    // after it, so the reader could not tell which command Enter would run.
    let (mut tree, _menu, mut reader) = focused_file_menu();
    for (key, item) in [
        (Key::ArrowDown, "New"),
        (Key::ArrowDown, "Open"),
        (Key::ArrowDown, "Save"),
        (Key::End, "Quit"),
        (Key::ArrowUp, "Save"),
        (Key::Home, "New"),
        (Key::PageDown, "Quit"),
        (Key::PageUp, "New"),
        (Key::S, "Save"),
        (Key::ArrowDown, "Quit"),
        (Key::ArrowDown, "New"),
    ] {
        press(&mut tree, key);
        assert_eq!(
            reader.heard(&mut tree),
            focus(item),
            "{key:?} must move the reader's focus to {item:?}"
        );
    }
}

#[test]
fn the_reader_follows_the_highlight_past_a_hidden_row() {
    // `item_when` hides a row from the arrows; the reader must never be sent
    // to a node the platform does not have.
    let mut tree = light_tree();
    let menu = tree.add(
        MenuList::new()
            .item(MenuItem::new(lit!("Stop")))
            .item_when(MenuItem::new(lit!("Start")), false)
            .item(MenuItem::new(lit!("Restart"))),
    );
    lay_out(&mut tree);
    tree.focus(menu);
    lay_out(&mut tree);
    let mut reader = Listener::attach(&mut tree);
    press(&mut tree, Key::ArrowDown);
    assert_eq!(reader.heard(&mut tree), focus("Stop"));
    press(&mut tree, Key::ArrowDown);
    assert_eq!(reader.heard(&mut tree), focus("Restart"));
}

/// Every item under the first `Role::Menu` an adapter reaches, as
/// `(name, posinset, setsize)`, the way AT-SPI and UIA read them: the
/// position on the item, the size from the nearest container that has one.
fn menu_items_as_read(tree: &mut WidgetTree) -> Vec<(String, Option<usize>, Option<usize>)> {
    fn find_menu<'a>(node: NodeRef<'a>) -> Option<NodeRef<'a>> {
        if node.role() == Role::Menu {
            return Some(node);
        }
        node.filtered_children(&common_filter).find_map(find_menu)
    }
    let platform = Tree::new(tree.sync_accessibility(), true);
    let state = platform.state();
    let menu = find_menu(state.root()).expect("a menu in the tree");
    menu.filtered_children(&common_filter)
        .filter(|n| {
            matches!(
                n.role(),
                Role::MenuItem | Role::MenuItemCheckBox | Role::MenuItemRadio
            )
        })
        .map(|n| {
            (
                n.label().unwrap_or_default(),
                // AccessKit counts from zero; both adapters add one back
                // (`accesskit_atspi_common` node.rs:393-395,
                // `accesskit_windows` node.rs:682-687).
                n.position_in_set().map(|p| p + 1),
                n.size_of_set_from_container(&common_filter),
            )
        })
        .collect()
}

#[test]
fn each_item_says_where_it_stands_in_its_menu() {
    // A separator is not an item, and a row hidden by `item_when` is not one
    // either while it is hidden, so neither counts.
    let gate = teksilo_core::signal::Signal::new(false);
    let mut tree = light_tree();
    tree.add(
        MenuList::new()
            .item(MenuItem::new(lit!("Cut")))
            .item(MenuItem::new(lit!("Copy")))
            .separator()
            .item_when(MenuItem::new(lit!("Paste Special")), gate.clone())
            .item(MenuItem::new(lit!("Paste"))),
    );
    lay_out(&mut tree);
    assert_eq!(
        menu_items_as_read(&mut tree),
        vec![
            ("Cut".to_string(), Some(1), Some(3)),
            ("Copy".to_string(), Some(2), Some(3)),
            ("Paste".to_string(), Some(3), Some(3)),
        ]
    );

    gate.set(true);
    lay_out(&mut tree);
    assert_eq!(
        menu_items_as_read(&mut tree),
        vec![
            ("Cut".to_string(), Some(1), Some(4)),
            ("Copy".to_string(), Some(2), Some(4)),
            ("Paste Special".to_string(), Some(3), Some(4)),
            ("Paste".to_string(), Some(4), Some(4)),
        ]
    );
}

// ── The menu's name ─────────────────────────────────────────────

fn tree_with_window() -> WidgetTree {
    use teksilo_core::window::state::WindowStateInit;
    use teksilo_core::window::{TeksiloWindowId, WindowPlacement, WindowState};
    let mut tree = light_tree();
    tree.set_window_state(WindowState::new(WindowStateInit {
        id: TeksiloWindowId::new(1),
        string_id: Some("test".to_string()),
        placement: WindowPlacement::Floating,
        title: "Test".to_string(),
        size: (800, 600),
        position: (0, 0),
        focused: true,
        resizable: true,
        always_on_top: false,
    }));
    tree
}

/// The node an adapter reports for `role` named `name`, if any.
fn trigger_named(tree: &mut WidgetTree, role: Role, name: &str) -> WidgetId {
    let update = tree.sync_accessibility();
    let (id, _) = update
        .nodes
        .iter()
        .find(|(_, node)| node.role() == role && node.label() == Some(name))
        .unwrap_or_else(|| panic!("no {role:?} named {name:?}"));
    teksilo_core::accessibility::node_id_to_widget_id_maybe(*id)
        .unwrap_or_else(|| panic!("{role:?} {name:?} is not a widget node"))
}

#[test]
fn a_menu_opened_from_the_menu_bar_is_named_after_its_trigger() {
    // Opening File, Edit or View was announced as "menu." and nothing else:
    // the Role::Menu node had no name and no labelled_by.
    let mut tree = tree_with_window();
    tree.add(
        crate::MenuBar::new()
            .menu(lit!("&File"), || {
                Box::new(
                    MenuList::new()
                        .item(MenuItem::new(lit!("New")))
                        .item(MenuItem::submenu(lit!("&Recent"), || {
                            Box::new(MenuList::new().item(MenuItem::new(lit!("notes.md"))))
                        })),
                )
            })
            .menu(lit!("&Edit"), || {
                Box::new(MenuList::new().item(MenuItem::new(lit!("Cut"))))
            }),
    );
    lay_out(&mut tree);
    let file = trigger_named(&mut tree, Role::MenuItem, "File");
    tree.focus(file);
    lay_out(&mut tree);
    let mut reader = Listener::attach(&mut tree);

    press(&mut tree, Key::ArrowDown);
    let heard = reader.heard(&mut tree);
    assert!(
        reader.finds(Role::Menu, "File"),
        "the open menu must be named after the File trigger"
    );
    assert_eq!(heard, focus("File"));

    press(&mut tree, Key::ArrowDown);
    assert_eq!(reader.heard(&mut tree), focus("New"));
    press(&mut tree, Key::ArrowDown);
    assert_eq!(reader.heard(&mut tree), focus("Recent"));

    press(&mut tree, Key::ArrowRight);
    let heard = reader.heard(&mut tree);
    assert!(
        reader.finds(Role::Menu, "Recent"),
        "a submenu must be named after the row that opened it"
    );
    assert_eq!(heard, focus("Recent"));

    press(&mut tree, Key::ArrowDown);
    assert_eq!(reader.heard(&mut tree), focus("notes.md"));
}

#[test]
fn a_menu_in_a_popover_is_named_after_its_button() {
    let mut tree = light_tree();
    tree.add(
        crate::PopoverButton::new(crate::Button::new(lit!("Add")))
            .content(MenuList::new().item(MenuItem::new(lit!("Rectangle")))),
    );
    lay_out(&mut tree);
    let add = trigger_named(&mut tree, Role::Button, "Add");
    tree.focus(add);
    lay_out(&mut tree);
    let mut reader = Listener::attach(&mut tree);
    press(&mut tree, Key::Enter);
    let heard = reader.heard(&mut tree);
    assert!(
        reader.finds(Role::Menu, "Add"),
        "the popover's menu must be named after the button that opened it"
    );
    assert_eq!(heard, focus("Add"));
    press(&mut tree, Key::ArrowDown);
    assert_eq!(reader.heard(&mut tree), focus("Rectangle"));
}

// ── A submenu the keyboard opened stays open ────────────────────

/// A menu whose first row opens a submenu, focused, with that row
/// highlighted.
fn menu_on_its_submenu_row() -> (WidgetTree, WidgetId) {
    let mut tree = light_tree();
    let menu = tree.add(
        MenuList::new()
            .item(MenuItem::submenu(lit!("Recent"), || {
                Box::new(
                    MenuList::new()
                        .item(MenuItem::new(lit!("project-alpha.toml")))
                        .item(MenuItem::new(lit!("notes.md"))),
                )
            }))
            .item(MenuItem::new(lit!("Quit"))),
    );
    tree.layout(SizeProposal::with_width(300.0));
    tree.focus(menu);
    tree.press_key(Key::ArrowDown, Modifiers::NONE);
    (tree, menu)
}

#[test]
fn a_submenu_opened_from_the_keyboard_outlives_a_resting_pointer() {
    // Right and Enter opened the submenu with a synthesised mouse click on
    // the row, so it took the dismissal a mouse-opened submenu takes: close
    // 150 ms after the pointer is anywhere but the row or the panel. A mouse
    // resting elsewhere on the window closed it about 165 ms after it
    // opened, and no recent file could be opened from the keyboard.
    for open in [Key::ArrowRight, Key::Enter, Key::Space] {
        let (mut tree, _menu) = menu_on_its_submenu_row();
        tree.press_key(open, Modifiers::NONE);
        tree.layout(SizeProposal::with_width(300.0));
        assert_eq!(
            tree.active_overlays().len(),
            1,
            "{open:?} must open the submenu"
        );

        // A mouse at rest somewhere else in the window, as a compositor
        // reports one with no button down.
        tree.pointer_move(Point::new(600.0, 500.0));
        tree.advance_time(Duration::from_secs(1));
        tree.layout(SizeProposal::with_width(300.0));
        assert_eq!(
            tree.active_overlays().len(),
            1,
            "a submenu {open:?} opened must stay open until Escape, Left or a choice"
        );
    }
}
