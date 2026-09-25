// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! What a screen reader gets from an open combo box, read through
//! `accesskit_consumer` as every platform adapter reads it.
//!
//! The screen-reader sweep found arrowing through an open list silent: the
//! keys moved a selection that no focus change or active descendant carried,
//! so Orca, NVDA and VoiceOver had nothing to follow. On Linux a second fault
//! hid the one channel left: a long list nested the options inside
//! `ListView` rows, so the list box the selection changed in answered AT-SPI's
//! Selection interface with no selected child. The arrows also committed the
//! value at every step, which switched an application's language as a reader
//! only listened to the options.

use super::*;
use crate::common::heard_test::{Heard, Listener};
use accesskit_consumer::{NodeRef, common_filter};
use teksilo_core::accesskit::Role;
use teksilo_core::event::Modifiers;
use teksilo_core::widget_tree::WidgetTree;

fn light_tree() -> WidgetTree {
    WidgetTree::new().with_theme(teksilo_core::presets::intui::light())
}

fn laid_out(tree: &mut WidgetTree) {
    tree.layout(SizeProposal::exact(400.0, 600.0));
}

fn focus(heard: &str) -> Heard {
    Heard::Focus(heard.to_string())
}

/// Forty items: more than `max_visible_items`, so the list is virtualized.
fn long_list() -> Vec<String> {
    (0..40).map(|i| format!("Item {i:02}")).collect()
}

fn walk<'a>(node: NodeRef<'a>, out: &mut Vec<NodeRef<'a>>) {
    for child in node.filtered_children(&common_filter) {
        out.push(child);
        walk(child, out);
    }
}

fn descendants(node: NodeRef<'_>) -> Vec<NodeRef<'_>> {
    let mut out = Vec::new();
    walk(node, &mut out);
    out
}

fn label(node: &NodeRef<'_>) -> String {
    node.label().unwrap_or_default()
}

/// A focused combo box and a reader attached to it.
fn focused_combo(tree: &mut WidgetTree, combo: ComboBox<String>) -> (WidgetId, Listener) {
    let id = tree.add(combo);
    laid_out(tree);
    tree.focus(id);
    laid_out(tree);
    let listener = Listener::attach(tree);
    (id, listener)
}

fn press(tree: &mut WidgetTree, key: Key, modifiers: Modifiers) {
    tree.press_key(key, modifiers);
    laid_out(tree);
}

#[test]
fn each_option_is_spoken_as_the_arrows_reach_it() {
    let mut tree = light_tree();
    let selected = Signal::new(None::<String>);
    let (_, mut reader) = focused_combo(
        &mut tree,
        ComboBox::new(["Apple", "Banana", "Cherry"], selected.clone()).label(lit!("Fruit")),
    );

    press(&mut tree, Key::ArrowDown, Modifiers::ALT);
    assert_eq!(
        reader.heard(&mut tree),
        vec![focus("Apple")],
        "opening the list names the option the keyboard is on"
    );
    press(&mut tree, Key::ArrowDown, Modifiers::NONE);
    assert_eq!(reader.heard(&mut tree), vec![focus("Banana")]);
    press(&mut tree, Key::ArrowDown, Modifiers::NONE);
    assert_eq!(reader.heard(&mut tree), vec![focus("Cherry")]);
    assert_eq!(
        selected.get(),
        None,
        "moving through the list commits nothing"
    );

    press(&mut tree, Key::Enter, Modifiers::NONE);
    assert_eq!(selected.get().as_deref(), Some("Cherry"), "Enter commits");
    assert!(
        reader.heard(&mut tree).contains(&focus("Fruit")),
        "focus comes back to the combo box, which now holds the value"
    );
}

#[test]
fn a_long_list_is_spoken_option_by_option_too() {
    let mut tree = light_tree();
    let selected = Signal::new(None::<String>);
    let (_, mut reader) = focused_combo(
        &mut tree,
        ComboBox::new(long_list(), selected.clone()).label(lit!("Huge")),
    );

    press(&mut tree, Key::ArrowDown, Modifiers::ALT);
    assert_eq!(reader.heard(&mut tree), vec![focus("Item 00")]);
    press(&mut tree, Key::ArrowDown, Modifiers::NONE);
    assert_eq!(reader.heard(&mut tree), vec![focus("Item 01")]);
    press(&mut tree, Key::End, Modifiers::NONE);
    assert_eq!(reader.heard(&mut tree), vec![focus("Item 39")]);
    press(&mut tree, Key::PageUp, Modifiers::NONE);
    assert_eq!(reader.heard(&mut tree), vec![focus("Item 31")]);

    press(&mut tree, Key::Enter, Modifiers::NONE);
    assert_eq!(selected.get().as_deref(), Some("Item 31"));
}

/// The list a reader walks: one list box, straight under the combo box that
/// controls it, holding one level of named options, the highlighted one the
/// only one selected.
fn assert_one_flat_list_box(reader: &Listener, highlighted: &str) {
    let state = reader.platform().state();
    let root = state.root();
    let all = descendants(root);
    let combo = *all
        .iter()
        .find(|n| n.role() == Role::ComboBox)
        .expect("a combo box in the tree");
    let list_boxes: Vec<_> = descendants(combo)
        .into_iter()
        .filter(|n| n.role() == Role::ListBox)
        .collect();
    assert_eq!(
        list_boxes.len(),
        1,
        "one list box under the combo box, not a list box inside a list box"
    );
    let list_box = list_boxes[0];

    let options: Vec<_> = descendants(list_box)
        .into_iter()
        .filter(|n| n.role() == Role::ListBoxOption)
        .collect();
    assert!(!options.is_empty(), "the open list shows options");
    for option in &options {
        assert!(
            !label(option).is_empty(),
            "every option is named; an unnamed option wraps {:?}",
            descendants(*option).iter().map(label).collect::<Vec<_>>()
        );
        assert!(
            descendants(*option)
                .iter()
                .all(|n| n.role() != Role::ListBoxOption),
            "option {:?} wraps another option",
            label(option)
        );
    }

    // What AT-SPI's Selection interface answers, and so what Orca speaks on
    // the list box's `selection-changed` (`default.py`, `onSelectionChanged`).
    let selected: Vec<String> = list_box
        .items(common_filter)
        .filter(|item| item.is_selected() == Some(true))
        .map(|item| label(&item))
        .collect();
    assert_eq!(selected, vec![highlighted.to_string()]);

    assert_eq!(
        list_box.filtered_parent(&common_filter).map(|p| p.id()),
        Some(combo.id()),
        "the list box hangs from the combo box, not from an unnamed node between them"
    );
    assert_eq!(
        combo.controls().map(|n| n.id()).collect::<Vec<_>>(),
        vec![list_box.id()],
        "the combo box controls its list box"
    );
}

#[test]
fn a_long_list_is_one_list_box_of_named_options() {
    let mut tree = light_tree();
    let selected = Signal::new(None::<String>);
    let (_, mut reader) = focused_combo(
        &mut tree,
        ComboBox::new(long_list(), selected.clone()).label(lit!("Huge")),
    );
    press(&mut tree, Key::ArrowDown, Modifiers::ALT);
    press(&mut tree, Key::ArrowDown, Modifiers::NONE);
    let _ = reader.heard(&mut tree);
    assert_one_flat_list_box(&reader, "Item 01");
}

#[test]
fn a_short_list_is_one_list_box_of_named_options() {
    let mut tree = light_tree();
    let selected = Signal::new(None::<String>);
    let (_, mut reader) = focused_combo(
        &mut tree,
        ComboBox::new(["Apple", "Banana", "Cherry"], selected.clone()).label(lit!("Fruit")),
    );
    press(&mut tree, Key::ArrowDown, Modifiers::ALT);
    press(&mut tree, Key::ArrowDown, Modifiers::NONE);
    let _ = reader.heard(&mut tree);
    assert_one_flat_list_box(&reader, "Banana");
}

#[test]
fn the_combo_box_never_names_an_option_the_tree_no_longer_holds() {
    // A wheel scrolls the highlighted row out of the realized window and the
    // list destroys it. Every adapter resolves a missing active descendant to
    // the focused node itself, but the tree must not name a node it dropped.
    let mut tree = light_tree();
    let labels: Vec<String> = (0..2_000).map(|i| format!("Row {i}")).collect();
    let (_, mut reader) = focused_combo(
        &mut tree,
        ComboBox::new(labels, Signal::new(None)).max_visible_items(8),
    );
    press(&mut tree, Key::ArrowDown, Modifiers::ALT);
    let row0 = tree
        .find_by_label("Row 0")
        .expect("the list opens on Row 0");
    tree.pointer_move(tree.bounds(row0).center());
    tree.dispatch_event(WidgetEvent::scroll(
        teksilo_core::event::ScrollDelta::Pixels { x: 0.0, y: 3_000.0 },
        Default::default(),
    ));
    tree.tick_animations(std::time::Duration::from_millis(200));
    laid_out(&mut tree);
    assert!(
        tree.find_by_label("Row 0").is_none(),
        "the highlighted row was scrolled out of the realized window"
    );

    let _ = reader.heard(&mut tree);
    let state = reader.platform().state();
    let combo = descendants(state.root())
        .into_iter()
        .find(|n| n.role() == Role::ComboBox)
        .expect("a combo box in the tree");
    assert_eq!(
        combo.data().active_descendant().is_some(),
        combo.active_descendant().is_some(),
        "the combo box names {:?} as its active descendant, a node the tree no longer holds",
        combo.data().active_descendant()
    );
}

#[test]
fn down_from_an_empty_combo_box_reaches_the_first_option() {
    let mut tree = light_tree();
    let selected = Signal::new(None::<String>);
    let (_, mut reader) = focused_combo(
        &mut tree,
        ComboBox::new(["Apple", "Banana", "Cherry"], selected.clone()).label(lit!("Fruit")),
    );

    press(&mut tree, Key::ArrowDown, Modifiers::NONE);
    assert_eq!(reader.heard(&mut tree), vec![focus("Apple")]);
    press(&mut tree, Key::Enter, Modifiers::NONE);
    assert_eq!(selected.get().as_deref(), Some("Apple"));
}

#[test]
fn only_a_commit_changes_the_value() {
    let mut tree = light_tree();
    let selected = Signal::new(Some("Apple".to_string()));
    let picks = Rc::new(RefCell::new(Vec::<String>::new()));
    let picks_h = picks.clone();
    let (_, _reader) = focused_combo(
        &mut tree,
        ComboBox::new(["Apple", "Banana", "Cherry"], selected.clone())
            .on_select(move |v: &String, _ctx| picks_h.borrow_mut().push(v.clone())),
    );

    // Opening on the value and pressing Enter picks nothing new.
    press(&mut tree, Key::ArrowDown, Modifiers::ALT);
    press(&mut tree, Key::Enter, Modifiers::NONE);
    assert!(
        picks.borrow().is_empty(),
        "Enter on the value is no commit: {picks:?}"
    );

    // Every way of moving: from the closed box and inside the open list.
    for key in [
        Key::ArrowDown,
        Key::ArrowDown,
        Key::ArrowUp,
        Key::End,
        Key::C,
    ] {
        press(&mut tree, key, Modifiers::NONE);
        assert_eq!(
            selected.get().as_deref(),
            Some("Apple"),
            "{key:?} moved the value"
        );
    }
    assert!(
        picks.borrow().is_empty(),
        "no commit while moving: {picks:?}"
    );

    press(&mut tree, Key::Escape, Modifiers::NONE);
    assert_eq!(
        selected.get().as_deref(),
        Some("Apple"),
        "Escape keeps the value"
    );
    assert!(picks.borrow().is_empty());

    press(&mut tree, Key::Home, Modifiers::NONE);
    press(&mut tree, Key::ArrowDown, Modifiers::NONE);
    press(&mut tree, Key::Enter, Modifiers::NONE);
    assert_eq!(selected.get().as_deref(), Some("Banana"));
    assert_eq!(*picks.borrow(), vec!["Banana".to_string()]);
}

#[test]
fn the_language_switcher_changes_the_language_only_on_enter() {
    use crate::common::locale_switch_test::speaking;
    let (_mgr, mut tree) = speaking("en-US");
    tree.add(crate::LanguageSwitcher::new().locales(vec![
        "en-US".parse().unwrap(),
        "fr-FR".parse().unwrap(),
        "ja-JP".parse().unwrap(),
    ]));
    laid_out(&mut tree);
    let combo = tree
        .find_by_role(Role::ComboBox)
        .expect("the switcher is a combo box");
    tree.focus(combo);
    laid_out(&mut tree);

    press(&mut tree, Key::ArrowDown, Modifiers::NONE);
    assert_eq!(
        tree.take_pending_locale_request(),
        None,
        "arrowing to a language must not switch to it"
    );
    press(&mut tree, Key::ArrowDown, Modifiers::NONE);
    assert_eq!(tree.take_pending_locale_request(), None);
    press(&mut tree, Key::Enter, Modifiers::NONE);
    assert_eq!(tree.take_pending_locale_request().as_deref(), Some("ja-JP"));
    teksilo_i18n::thread_local::clear();
}

#[test]
fn the_search_field_is_named_and_its_arrows_speak_the_matches() {
    let mut tree = light_tree();
    let selected = Signal::new(None::<String>);
    let countries = vec![
        "Belgium".to_string(),
        "France".to_string(),
        "Germany".to_string(),
    ];
    let (combo, mut reader) = focused_combo(
        &mut tree,
        ComboBox::new(countries, selected.clone())
            .label(lit!("Country"))
            .searchable(true),
    );

    press(&mut tree, Key::Enter, Modifiers::NONE);
    assert_eq!(
        reader.heard(&mut tree),
        vec![focus("Country")],
        "focus moves into a search field named after the combo box"
    );

    tree.type_text(combo, "fr");
    laid_out(&mut tree);
    let _ = reader.heard(&mut tree);
    press(&mut tree, Key::ArrowDown, Modifiers::NONE);
    assert_eq!(reader.heard(&mut tree), vec![focus("France")]);
    assert_eq!(
        selected.get(),
        None,
        "moving through the matches commits nothing"
    );

    press(&mut tree, Key::Enter, Modifiers::NONE);
    assert_eq!(selected.get().as_deref(), Some("France"));
}

#[test]
fn typing_after_an_arrow_puts_the_reader_back_in_the_search_field() {
    // An option named as the field's active descendant is the reader's focus,
    // and the field is not: a screen reader then has no field to echo the
    // typed characters in. Typing starts a new search, so the highlight goes.
    let mut tree = light_tree();
    let selected = Signal::new(None::<String>);
    let (combo, mut reader) = focused_combo(
        &mut tree,
        ComboBox::new(["Belgium", "France", "Germany"], selected.clone())
            .label(lit!("Country"))
            .searchable(true),
    );
    press(&mut tree, Key::Enter, Modifiers::NONE);
    press(&mut tree, Key::ArrowDown, Modifiers::NONE);
    assert_eq!(reader.heard(&mut tree).last(), Some(&focus("Belgium")));

    tree.type_text(combo, "g");
    laid_out(&mut tree);
    let heard = reader.heard(&mut tree);
    assert!(
        heard.contains(&focus("Country")),
        "focus is back on the search field: {heard:?}"
    );
    press(&mut tree, Key::ArrowDown, Modifiers::NONE);
    assert_eq!(
        reader.heard(&mut tree),
        vec![focus("Belgium")],
        "the arrow starts again from the first match"
    );
}

#[test]
fn a_long_searchable_list_is_one_list_box_of_named_options() {
    let mut tree = light_tree();
    let selected = Signal::new(None::<String>);
    let (_, mut reader) = focused_combo(
        &mut tree,
        ComboBox::new(long_list(), selected.clone())
            .label(lit!("Huge"))
            .searchable(true),
    );
    press(&mut tree, Key::Enter, Modifiers::NONE);
    press(&mut tree, Key::ArrowDown, Modifiers::NONE);
    press(&mut tree, Key::ArrowDown, Modifiers::NONE);
    assert_eq!(reader.heard(&mut tree).last(), Some(&focus("Item 01")));

    let state = reader.platform().state();
    let entry = descendants(state.root())
        .into_iter()
        .find(|n| n.is_text_input())
        .expect("a search field");
    assert_eq!(label(&entry), "Huge", "the search field is named");
    assert_one_flat_list_box(&reader, "Item 01");
}
