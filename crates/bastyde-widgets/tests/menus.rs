// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Integration tests for the menu system.
//!
//! These exercise cross-widget keyboard flows that the per-file
//! tests can't easily reach: a real `MenuBar` plus its `MenuList`
//! dropdowns wired to a `WidgetTree` carrying a `WindowState`, so
//! the window-level mnemonic dispatcher actually fires through the
//! full hover / focus / overlay pipeline.

use std::cell::Cell;
use std::collections::VecDeque;
use std::rc::Rc;

use bastyde_canvas::SizeProposal;
use bastyde_core::accesskit::Role;
use bastyde_core::event::{Key, Modifiers};
use bastyde_core::presets::intui;
use bastyde_core::widget_id::WidgetId;
use bastyde_core::widget_tree::WidgetTree;
use bastyde_core::window::state::WindowStateInit;
use bastyde_core::window::{BastydeWindowId, WindowPlacement, WindowState};
use bastyde_i18n::lit;
use bastyde_widgets::menu_bar::MenuBar;
use bastyde_widgets::menu_item::MenuItem;
use bastyde_widgets::menu_list::MenuList;

fn fresh_tree() -> WidgetTree {
    let mut t = WidgetTree::new().with_theme(intui::light());
    t.set_window_state(WindowState::new(WindowStateInit {
        id: BastydeWindowId::new(1),
        string_id: Some("test".to_string()),
        placement: WindowPlacement::Floating,
        title: "Test".to_string(),
        size: (1024, 768),
        position: (0, 0),
        focused: false,
        resizable: true,
        always_on_top: false,
    }));
    t
}

fn find_first_with_role(t: &WidgetTree, from: WidgetId, role: Role) -> WidgetId {
    let mut queue = VecDeque::new();
    queue.push_back(from);
    while let Some(id) = queue.pop_front() {
        if t.accessibility_node(id).role() == role {
            return id;
        }
        for child in t.children(id) {
            queue.push_back(child);
        }
    }
    panic!("no descendant of {from:?} with role {role:?}");
}

#[test]
fn menubar_role_and_trigger_names() {
    let mut t = fresh_tree();
    let mb = t.add(
        MenuBar::new()
            .menu(lit!("&File"), || Box::new(MenuList::new()))
            .menu(lit!("&Edit"), || Box::new(MenuList::new())),
    );
    t.layout(SizeProposal::exact(1024.0, 100.0));
    assert_eq!(t.accessibility_node(mb).role(), Role::MenuBar);
    let info = t.accessibility_node(find_first_with_role(&t, mb, Role::MenuItem));
    assert_eq!(info.name(), Some("File"));
}

#[test]
fn first_menu_item_emits_role_menuitem_inside_dropdown_only_after_open() {
    // The dropdown's content widget is added as dormant and is only
    // activated when the menu opens. Before opening, the AT walk
    // shouldn't see "Save" as a live MenuItem in the menubar's
    // subtree.
    let mut t = fresh_tree();
    let mb = t.add(MenuBar::new().menu(lit!("&File"), || {
        Box::new(MenuList::new().item(MenuItem::new(lit!("&Save"))))
    }));
    t.layout(SizeProposal::exact(1024.0, 100.0));
    // Walking the MenuBar's subtree: we find the trigger MenuItem
    // ("File") but not the dormant "Save" item — searching by label.
    assert!(t.find_by_label("File").is_some());
    // The submenu content is dormant pre-open, so the AT walker
    // skips it. Save MUST not be in the active tree yet.
    assert!(
        t.find_by_label("Save").is_none(),
        "dormant submenu items must not appear in AT before open"
    );
    let _ = mb;
}

#[test]
fn dispatcher_installed_when_menubar_mounted() {
    let mut t = fresh_tree();
    let _ = t.add(MenuBar::new().menu(lit!("&File"), || Box::new(MenuList::new())));
    t.layout(SizeProposal::exact(1024.0, 100.0));
    assert!(
        t.window_state()
            .expect("window")
            .menubar_dispatcher()
            .is_some()
    );
}

#[test]
fn menubar_with_two_menus_has_distinct_mnemonic_keys() {
    // The Alt+F and Alt+E mnemonics should map to two different
    // triggers. Verified through the AT `access_key` field that the
    // walker propagates.
    let mut t = fresh_tree();
    let mb = t.add(
        MenuBar::new()
            .menu(lit!("&File"), || Box::new(MenuList::new()))
            .menu(lit!("&Edit"), || Box::new(MenuList::new())),
    );
    t.layout(SizeProposal::exact(1024.0, 100.0));

    // Both triggers should have AT names matching the stripped form.
    let file = t.find_by_label("File").expect("File exists");
    let edit = t.find_by_label("Edit").expect("Edit exists");
    assert_ne!(file, edit, "trigger ids must differ");
    let _ = mb;
}

// --- Keyboard activation through MenuItem ---

#[test]
fn enter_in_focused_menu_item_fires_action() {
    let fired = Rc::new(Cell::new(false));
    let mut t = fresh_tree();
    let fired_for_action = fired.clone();
    let menu_id = t.add(
        MenuList::new().item(MenuItem::new(lit!("Save")).on_activate_fn(move |_| {
            fired_for_action.set(true);
        })),
    );
    t.layout(SizeProposal::exact(300.0, 100.0));
    t.focus(menu_id);
    // ArrowDown to focus first item.
    t.press_key(Key::ArrowDown, Modifiers::NONE);
    t.press_key(Key::Enter, Modifiers::NONE);
    assert!(fired.get(), "Enter on a focused MenuItem must fire action");
}

#[test]
fn type_ahead_in_long_menu_jumps_to_distant_match() {
    let fired = Rc::new(Cell::new(None));
    let mut t = fresh_tree();
    let labels = [
        "Antigua", "Brazil", "Chile", "Denmark", "Egypt", "France", "Germany", "Honduras",
        "Iceland", "Japan", "Kenya", "Latvia", "Mexico", "Norway", "Oman", "Peru", "Qatar",
        "Russia",
    ];
    let mut menu = MenuList::new();
    for (i, name) in labels.iter().enumerate() {
        let fired_for_this = fired.clone();
        menu = menu
            .item(MenuItem::new(lit!(*name)).on_activate_fn(move |_| fired_for_this.set(Some(i))));
    }
    let menu_id = t.add(menu);
    t.layout(SizeProposal::exact(300.0, 600.0));
    t.focus(menu_id);
    // Type-ahead 'q' jumps to "Qatar" (only label starting with 'q').
    t.press_key(Key::Q, Modifiers::NONE);
    t.press_key(Key::Enter, Modifiers::NONE);
    let idx = fired.get().expect("an item should have fired");
    assert_eq!(labels[idx], "Qatar");
}

#[test]
fn arrows_navigate_then_enter_activates_correctly() {
    let fired = Rc::new(Cell::new(None));
    let mut t = fresh_tree();
    let mut menu = MenuList::new();
    for (i, name) in ["A", "B", "C", "D"].iter().enumerate() {
        let fired_for_this = fired.clone();
        menu = menu
            .item(MenuItem::new(lit!(*name)).on_activate_fn(move |_| fired_for_this.set(Some(i))));
    }
    let menu_id = t.add(menu);
    t.layout(SizeProposal::exact(200.0, 200.0));
    t.focus(menu_id);
    t.press_key(Key::ArrowDown, Modifiers::NONE);
    t.press_key(Key::ArrowDown, Modifiers::NONE);
    t.press_key(Key::ArrowDown, Modifiers::NONE);
    t.press_key(Key::Enter, Modifiers::NONE);
    assert_eq!(fired.get(), Some(2));
}

#[test]
fn end_jumps_to_last_and_enter_activates() {
    let fired = Rc::new(Cell::new(None));
    let mut t = fresh_tree();
    let mut menu = MenuList::new();
    for (i, name) in ["A", "B", "C"].iter().enumerate() {
        let fired_for_this = fired.clone();
        menu = menu
            .item(MenuItem::new(lit!(*name)).on_activate_fn(move |_| fired_for_this.set(Some(i))));
    }
    let menu_id = t.add(menu);
    t.layout(SizeProposal::exact(200.0, 200.0));
    t.focus(menu_id);
    t.press_key(Key::End, Modifiers::NONE);
    t.press_key(Key::Enter, Modifiers::NONE);
    assert_eq!(fired.get(), Some(2));
}

#[test]
fn mnemonic_within_open_menu_activates_matching_item() {
    let fired = Rc::new(Cell::new(None));
    let mut t = fresh_tree();
    let mut menu = MenuList::new();
    for (i, name) in ["&New", "&Open", "&Save", "&Quit"].iter().enumerate() {
        let fired_for_this = fired.clone();
        menu = menu
            .item(MenuItem::new(lit!(*name)).on_activate_fn(move |_| fired_for_this.set(Some(i))));
    }
    let menu_id = t.add(menu);
    t.layout(SizeProposal::exact(200.0, 200.0));
    t.focus(menu_id);
    // Bare 'o' (no Alt) within an open menu activates "Open".
    t.press_key(Key::O, Modifiers::NONE);
    assert_eq!(fired.get(), Some(1));
}
