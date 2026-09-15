// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! The build-breaking half of the drift guard.
//!
//! Both tests read `teksilo-core`'s `widget_builder.rs` and
//! `teksilo-parse`'s `diag.rs` from source, so neither can pass by agreeing
//! with a copy of the list kept here. See the crate docs for the failure mode
//! the first test prevents.

use std::fs;

use teksilo_teksu_guard as guard;

fn core_source() -> String {
    let path = guard::widget_builder_source_path();
    fs::read_to_string(&path).unwrap_or_else(|e| panic!("cannot read {}: {e}", path.display()))
}

fn diag_source() -> String {
    let path = guard::diag_source_path();
    fs::read_to_string(&path).unwrap_or_else(|e| panic!("cannot read {}: {e}", path.display()))
}

/// Every `WidgetBuilder` method that wraps its receiver in
/// `WidgetWithHandlers<Self>` is known to the `teksu!` lowering pass.
///
/// A name missing here is not a missing feature — the method still works in a
/// plain builder chain. It is a `teksu!` body that stops compiling as soon as
/// the property is followed by a child or a widget-specific setter, with a
/// diagnostic naming `.child`, which the user did not write.
#[test]
fn every_wrapping_builder_method_is_known_to_the_dsl() {
    let missing = guard::missing_from_predicate(
        &core_source(),
        teksilo_parse::diag::is_widget_builder_method,
    );

    assert!(
        missing.is_empty(),
        "these `WidgetBuilder` methods return `WidgetWithHandlers<Self>` but are absent \
         from `teksilo_parse::diag::is_widget_builder_method`, so `teksu!` will not move \
         them to the end of the chain and any child after them fails to resolve:\n{}\n\
         Add each name to the `matches!` list in crates/teksilo-parse/src/diag.rs.",
        missing
            .iter()
            .map(|n| format!("  - {n}"))
            .collect::<Vec<_>>()
            .join("\n"),
    );
}

/// Every name the predicate accepts still resolves to a method on the
/// `WidgetBuilder` trait or on `impl WidgetWithHandlers<W>`.
///
/// The reverse drift: a rename in core leaves the old name in the list, where
/// it goes on being reordered forever and the new name is not.
#[test]
fn the_dsl_lists_no_method_that_no_longer_exists() {
    let listed = guard::predicate_listed_names(&diag_source());
    assert!(
        !listed.is_empty(),
        "extracted no names from `is_widget_builder_method` — the guard has lost its subject"
    );

    let stale = guard::stale_in_predicate(
        &core_source(),
        listed.iter().map(String::as_str).collect::<Vec<_>>(),
    );

    assert!(
        stale.is_empty(),
        "these names are listed in `teksilo_parse::diag::is_widget_builder_method` but name \
         no method on `WidgetBuilder` nor on `impl WidgetWithHandlers<W>`:\n{}",
        stale
            .iter()
            .map(|n| format!("  - {n}"))
            .collect::<Vec<_>>()
            .join("\n"),
    );
}

/// The predicate the first test calls and the literals the second test reads
/// are the same list.
///
/// Without this, a literal the `matches!` never actually tests (a name in a
/// comment, a dead arm) would be admitted as coverage by the staleness check
/// while the behavioural check knew nothing about it.
#[test]
fn the_extracted_names_are_the_names_the_predicate_accepts() {
    for name in guard::predicate_listed_names(&diag_source()) {
        assert!(
            teksilo_parse::diag::is_widget_builder_method(&name),
            "`{name}` was extracted from diag.rs but the predicate rejects it"
        );
    }
}

/// No `WidgetBuilder` method may return a widget wrapper other than
/// `WidgetWithHandlers<Self>`.
///
/// The reorder rule repairs `-> WidgetWithHandlers<Self>`; it cannot repair a
/// method returning a different wrapper, because the rest of the chain then
/// resolves against that wrapper and usually still compiles. `dim_when_inactive`
/// did this and silently discarded the receiver and every child but the last.
/// See [`teksilo_teksu_guard::foreign_wrapper_returns`].
#[test]
fn no_widget_builder_method_returns_a_foreign_wrapper() {
    let source = std::fs::read_to_string(teksilo_teksu_guard::widget_builder_source_path())
        .expect("widget_builder.rs is readable");
    let offenders = teksilo_teksu_guard::foreign_wrapper_returns(&source);
    assert!(
        offenders.is_empty(),
        "these `WidgetBuilder` methods return a wrapper the DSL's reorder rule \
         cannot repair, so a builder chain past them silently retargets and \
         builds a different tree: {offenders:?}. Give the wrapper its own \
         `Wrapper::new().child(w)` constructor and drop the trait method, the \
         way `Fade`, `Blur`, `Scale` and `Collapse` already do."
    );
}
