// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Keyboard tests for a [`ListView`] whose rows carry a checkbox.
use super::tests::FixedLeaf;
use super::*;
use teksilo_core::WidgetTree;
use teksilo_core::event::{Key, Modifiers};
use teksilo_data::{CheckedModel, SelectionMode, SelectionModel};
use teksilo_i18n::lit;

/// A 200-row list showing ~10, every row carrying a checkbox.
fn checked_list() -> (WidgetTree, SelectionModel, CheckedModel, SizeProposal) {
    let model = ListModel::from_vec((0..200usize).collect());
    let checks = CheckedModel::new();
    let selection = SelectionModel::new(SelectionMode::Multi);
    let (sel, ck) = (selection.clone(), checks.clone());
    let mut tree = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
    let lv = tree.add(
        ListView::new(model, move |i, item, selected| {
            Box::new(
                crate::StandardListItem::new(lit!(format!("Row {item}")))
                    .selected(selected)
                    .checkbox(ck.signal_for(i)),
            )
        })
        .item_height(24.0)
        .selection(sel),
    );
    let p = SizeProposal::exact(400.0, 240.0);
    tree.layout(p);
    tree.focus(lv);
    (tree, selection, checks, p)
}

#[test]
fn a_list_is_one_tab_stop_however_many_rows_are_realized() {
    // The row checkboxes used to be Tab stops of their own, which made the
    // Tab order a function of the virtualization window: 31 stops in this
    // fixture, and a *different* 31 after scrolling. A listbox is one Tab
    // stop with a cursor moving inside it.
    let (mut tree, _sel, _ck, p) = checked_list();
    let mut seen = std::collections::BTreeSet::new();
    for _ in 0..40 {
        tree.press_key(Key::Tab, Modifiers::NONE);
        tree.layout(p);
        seen.insert(tree.focused());
    }
    assert_eq!(
        seen.len(),
        1,
        "Tab must not walk into the rows; got {} distinct stops",
        seen.len()
    );
}

#[test]
fn space_checks_the_focused_row_and_ctrl_space_still_selects() {
    let (mut tree, sel, checks, p) = checked_list();
    sel.select(3);
    tree.layout(p);

    // Space reaches the checkbox — the row's only keyboard route to it,
    // now that it is out of the Tab order.
    tree.press_key(Key::Space, Modifiers::NONE);
    tree.layout(p);
    assert!(checks.signal_for(3).get(), "Space checks the focused row");
    assert_eq!(sel.selected_indices(), vec![3], "and leaves the selection");

    tree.press_key(Key::Space, Modifiers::NONE);
    tree.layout(p);
    assert!(!checks.signal_for(3).get(), "and unchecks it again");

    // Ctrl+Space keeps meaning "toggle the selection".
    tree.press_key(Key::Space, Modifiers::CTRL);
    tree.layout(p);
    assert_eq!(sel.selected_indices(), Vec::<usize>::new());
    assert!(!checks.signal_for(3).get(), "the check is untouched");
}

#[test]
fn a_row_without_a_checkbox_keeps_space_on_the_selection() {
    let model = ListModel::from_vec((0..20usize).collect());
    let selection = SelectionModel::new(SelectionMode::Multi);
    let sel = selection.clone();
    let mut tree = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
    let lv = tree.add(
        ListView::new(model, move |_i, _item, _s| Box::new(FixedLeaf(100.0, 24.0)))
            .item_height(24.0)
            .selection(sel),
    );
    let p = SizeProposal::exact(400.0, 240.0);
    tree.layout(p);
    tree.focus(lv);
    selection.select(2);

    tree.press_key(Key::Space, Modifiers::NONE);
    tree.layout(p);
    assert_eq!(
        selection.selected_indices(),
        Vec::<usize>::new(),
        "no checkbox to publish a toggle, so Space is still the selection"
    );
}
