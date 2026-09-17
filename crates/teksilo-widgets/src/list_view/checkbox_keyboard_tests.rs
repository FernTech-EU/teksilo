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
    //
    // Read off the traversal graph rather than by pressing Tab and counting
    // where focus lands. The fixture focuses the view directly and
    // `WidgetTree::focus` has no focusable guard, so that count is 1 both when
    // the list is a single stop and when it is no stop at all — the shape that
    // survived deleting `focusable(true)` elsewhere in this workspace.
    let (tree, _sel, _ck, _p) = checked_list();
    let stops = tree.tab_stops_within(tree.roots()[0]);
    assert_eq!(
        stops.len(),
        1,
        "a listbox is one Tab stop; got {} — 0 means no keyboard user can \
         reach the list at all, more than 1 means a row control has leaked \
         into the Tab order, where its presence tracks the scroll position",
        stops.len()
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
fn space_on_a_row_fires_the_checkbox_on_change() {
    // The path this test exists for: a checkbox inside a data-view row is out
    // of the Tab order, so `Space` on the focused row is its only keyboard
    // route. That route runs the row's published `keyboard_toggle`, which used
    // to be a bare `Fn()` with no context — so a checkbox `on_change` would
    // have fired under the pointer and stayed silent here, which is the worst
    // kind of hole: invisible, and only in the embedding the framework
    // advertises most. The marker carries an `EventContext` now.
    use std::cell::RefCell;
    use std::rc::Rc;

    let model = ListModel::from_vec((0..20usize).collect());
    let checks = CheckedModel::new();
    let selection = SelectionModel::new(SelectionMode::Multi);
    let (sel, ck) = (selection.clone(), checks.clone());
    let seen: Rc<RefCell<Vec<bool>>> = Rc::new(RefCell::new(Vec::new()));
    let sink = seen.clone();

    let mut tree = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
    let lv = tree.add(
        ListView::new(model, move |i, _item, _selected| {
            let sink = sink.clone();
            Box::new(
                crate::checkbox::Checkbox::new(ck.signal_for(i))
                    .label(lit!("Row"))
                    .on_change(move |now, _ctx| sink.borrow_mut().push(now)),
            )
        })
        .item_height(24.0)
        .selection(sel),
    );
    let p = SizeProposal::exact(400.0, 240.0);
    tree.layout(p);
    tree.focus(lv);
    selection.select(2);
    tree.layout(p);

    tree.press_key(Key::Space, Modifiers::NONE);
    tree.layout(p);
    assert!(checks.signal_for(2).get(), "Space checked the focused row");
    assert_eq!(
        *seen.borrow(),
        vec![true],
        "and the checkbox reported it through on_change"
    );

    tree.press_key(Key::Space, Modifiers::NONE);
    tree.layout(p);
    assert_eq!(*seen.borrow(), vec![true, false]);
}

#[test]
fn standard_list_item_forwards_on_checkbox_toggle() {
    // `StandardListItem` builds its checkbox internally, so without this
    // forwarder the canonical row is the one place `Checkbox::on_change` is
    // unreachable — an app would have to re-implement the row to get it.
    use std::cell::RefCell;
    use std::rc::Rc;

    let model = ListModel::from_vec((0..20usize).collect());
    let checks = CheckedModel::new();
    let selection = SelectionModel::new(SelectionMode::Multi);
    let (sel, ck) = (selection.clone(), checks.clone());
    let seen: Rc<RefCell<Vec<bool>>> = Rc::new(RefCell::new(Vec::new()));
    let sink = seen.clone();

    let mut tree = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
    let lv = tree.add(
        ListView::new(model, move |i, item, selected| {
            let sink = sink.clone();
            Box::new(
                crate::StandardListItem::new(lit!(format!("Row {item}")))
                    .selected(selected)
                    .checkbox(ck.signal_for(i))
                    .on_checkbox_toggle(move |now, _ctx| sink.borrow_mut().push(now)),
            )
        })
        .item_height(24.0)
        .selection(sel),
    );
    let p = SizeProposal::exact(400.0, 240.0);
    tree.layout(p);
    tree.focus(lv);
    selection.select(1);
    tree.layout(p);

    tree.press_key(Key::Space, Modifiers::NONE);
    tree.layout(p);
    assert!(checks.signal_for(1).get());
    assert_eq!(*seen.borrow(), vec![true]);
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
