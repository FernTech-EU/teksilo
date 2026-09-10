// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Spec §9.2: a bare child inside a Category B widget is pre-empted with a
//! hint naming the slot to use.
//!
//! The pre-empt is decided by NAME (`diag::is_category_b_widget`), which is
//! why the list has to name types that exist: a name that no widget answers
//! to costs the user the hint and hands them the generic "no method named
//! `child`" error instead. That is not a hypothetical — the list carried
//! `Popover`, which is not a type, while the four names the popover family
//! actually uses were all absent.
//!
//! These are the first tests in this crate. The end-to-end proof lives in
//! `crates/teksilo/tests/teksi/fail/`, whose fixtures reach the real
//! compiler; these reach the decision itself, in seconds rather than
//! minutes, so the list can be mutation-checked cheaply.

use teksilo_parse::{diag, parse_root};

/// Parse a bare element and return the error message, if it failed.
fn hint_for(source: &str) -> Option<String> {
    let tokens: proc_macro2::TokenStream = source.parse().expect("the fixture tokenizes");
    parse_root(tokens).err().map(|e| e.to_string())
}

/// Every name the pre-empt claims, with the slot it must name.
///
/// The popover entries are four separate rows on purpose: they are aliases of
/// one generic type, so nothing in the code would notice if three of them
/// were dropped, and the failure would be invisible to the one that stayed.
const CATEGORY_B: &[(&str, &str)] = &[
    ("Card", "content"),
    ("Accordion", "content"),
    ("TitleBar", "leading"),
    ("DialogContent", "body"),
    ("Breadcrumb", "item"),
    ("TabWidget", "tab"),
    ("PopoverWidget", "content"),
    ("PopoverButton", "content"),
    ("PopoverIconButton", "content"),
    ("PopoverCustom", "content"),
    ("Snackbar", "content"),
    ("Dialog", "content"),
    ("Wizard", "step"),
];

#[test]
fn every_category_b_widget_pre_empts_a_bare_child_with_its_own_slot() {
    for (widget, slot) in CATEGORY_B {
        let message = hint_for(&format!("{widget} {{ TextWidget(\"hi\") }}"))
            .unwrap_or_else(|| panic!("`{widget}` accepted a bare child instead of pre-empting it"));
        assert!(
            message.contains("Category B widget with named slots"),
            "`{widget}` failed for some other reason: {message}"
        );
        assert!(
            message.contains(&format!("use `{slot}: <widget>`")),
            "`{widget}` should point at its `{slot}` slot: {message}"
        );
    }
}

/// The predicate and the hint table have to agree, or a listed widget gets
/// pointed at the fallback slot rather than its own.
#[test]
fn the_predicate_and_the_slot_table_cover_the_same_names() {
    for (widget, _) in CATEGORY_B {
        assert!(
            diag::is_category_b_widget(widget),
            "`{widget}` is in this test's table but not in the predicate"
        );
    }
}

/// A container that takes children *is* allowed a bare child, which is what
/// makes the test above measure the pre-empt rather than a parse failure
/// every element shares.
#[test]
fn a_child_taking_container_is_left_alone() {
    assert!(
        hint_for("VStack { TextWidget(\"hi\") }").is_none(),
        "a bare child in a child-taking container must parse"
    );
    assert!(!diag::is_category_b_widget("VStack"));
}

/// A Category B widget addressing its slot by name is correct usage and must
/// not be pre-empted — the pre-empt keys on the bare child, not on the type.
#[test]
fn naming_the_slot_is_not_pre_empted() {
    assert!(
        hint_for("Card { content: TextWidget(\"hi\") }").is_none(),
        "`content:` is the very thing the hint asks for"
    );
}
