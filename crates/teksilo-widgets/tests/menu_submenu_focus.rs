// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Where keyboard focus is after a submenu opens.
//!
//! The native convention (Win32, Qt, AppKit): a submenu the pointer opens by
//! resting on its row leaves the keyboard in the menu the row belongs to, and
//! the keyboard enters the submenu only when the user asks for it (Right,
//! Enter or Space on the row, an assistive technology's action, a click).

use std::time::Duration;

use teksilo_canvas::SizeProposal;
use teksilo_core::accesskit::Role;
use teksilo_core::event::{Key, Modifiers};
use teksilo_core::presets::intui;
use teksilo_core::widget_id::WidgetId;
use teksilo_core::widget_tree::WidgetTree;
use teksilo_i18n::lit;
use teksilo_widgets::menu_item::MenuItem;
use teksilo_widgets::menu_list::MenuList;
use teksilo_widgets::primitives::{FixedSize, VStack};

fn size() -> SizeProposal {
    SizeProposal::exact(1024.0, 768.0)
}

/// A menu whose first row opens a submenu and whose second row is plain,
/// laid out narrow enough for the submenu to open on its trailing side.
fn menu() -> (WidgetTree, WidgetId) {
    let mut tree = WidgetTree::new().with_theme(intui::light());
    let menu = tree.add(
        MenuList::new()
            .item(MenuItem::submenu(lit!("More"), || {
                Box::new(
                    MenuList::new()
                        .item(MenuItem::new(lit!("Sub One")).on_activate_fn(|_| {}))
                        .item(MenuItem::new(lit!("Sub Two")).on_activate_fn(|_| {})),
                )
            }))
            .item(MenuItem::new(lit!("Second")).on_activate_fn(|_| {})),
    );
    tree.add(VStack::new().child(FixedSize::new().width(220.0).child(menu)));
    tree.layout(size());
    (tree, menu)
}

/// The rows of `menu`, top to bottom.
fn rows(tree: &WidgetTree, menu: WidgetId) -> Vec<WidgetId> {
    let mut stack = vec![menu];
    let mut out = Vec::new();
    while let Some(id) = stack.pop() {
        for child in tree.children(id) {
            if tree
                .widget_type_name(child)
                .is_some_and(|name| name.contains("MenuItem"))
            {
                out.push(child);
            }
            stack.push(child);
        }
    }
    out.sort_by(|a, b| tree.bounds(*a).y.total_cmp(&tree.bounds(*b).y));
    out
}

/// Rest the pointer on the submenu row until its open delay has passed.
fn hover_open(tree: &mut WidgetTree, menu: WidgetId) -> WidgetId {
    let trigger = rows(tree, menu)[0];
    tree.pointer_move(tree.bounds(trigger).center());
    tree.advance_time(Duration::from_millis(450));
    tree.layout(size());
    assert_eq!(
        tree.active_overlays().len(),
        1,
        "the submenu opens once the hover delay has passed"
    );
    tree.overlay_manager().active_content_ids()[0]
}

fn press(tree: &mut WidgetTree, key: Key) {
    tree.press_key(key, Modifiers::NONE);
    tree.layout(size());
}

/// The node the accessibility update names as focus, resolved through the
/// focused node's active descendant as the platform adapters resolve it.
fn reported_focus_label(tree: &mut WidgetTree) -> Option<String> {
    let update = tree.sync_accessibility();
    let nodes: std::collections::HashMap<_, _> = update.nodes.into_iter().collect();
    let focus = nodes.get(&update.focus)?;
    let target = focus
        .active_descendant()
        .and_then(|id| nodes.get(&id))
        .unwrap_or(focus);
    target.label().map(str::to_string)
}

#[test]
fn a_submenu_opened_by_hover_leaves_focus_in_the_parent_menu() {
    let (mut tree, menu) = menu();
    tree.focus(menu);
    let submenu = hover_open(&mut tree, menu);
    assert_eq!(
        tree.focused(),
        Some(menu),
        "resting the pointer on a row moves no keyboard focus"
    );
    assert!(
        !tree
            .focused()
            .is_some_and(|f| tree.is_descendant_of(f, submenu)),
        "focus is not inside the submenu"
    );
    // The arrows still move in the parent menu.
    press(&mut tree, Key::ArrowDown);
    assert_eq!(tree.focused(), Some(menu));
    assert_eq!(reported_focus_label(&mut tree).as_deref(), Some("More"));
    press(&mut tree, Key::ArrowDown);
    assert_eq!(reported_focus_label(&mut tree).as_deref(), Some("Second"));
}

#[test]
fn right_enters_a_submenu_the_pointer_opened() {
    let (mut tree, menu) = menu();
    tree.focus(menu);
    let submenu = hover_open(&mut tree, menu);
    // The keyboard highlight is on the submenu row, then Right enters.
    press(&mut tree, Key::ArrowDown);
    press(&mut tree, Key::ArrowRight);
    let focused = tree.focused().expect("something has focus");
    assert!(
        tree.is_descendant_of(focused, submenu),
        "Right moves focus into the submenu"
    );
    assert_eq!(tree.accessibility_node(focused).role(), Role::Menu);
}

#[test]
fn a_submenu_opened_by_hover_focuses_nothing_when_nothing_had_focus() {
    let (mut tree, menu) = menu();
    hover_open(&mut tree, menu);
    assert_eq!(tree.focused(), None, "a hover is not a request for focus");
}
