// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Structural invariants over the shipped framework translations.
//!
//! `teksilo-widgets` ships its own user-facing strings — accessibility names,
//! MessageBox buttons, the GDPR privacy panel, calendar names — as Fluent
//! bundles under `locales/`, registered by
//! [`teksilo_widgets::framework_locales`].
//!
//! Nothing else checks them. `tr_widget!` validates call sites against the
//! **source** `en-US.ftl` at compile time, and `I18nManager::resolve_widget`
//! falls back to en-US at runtime for any key a translation lacks. Both of
//! those are working as designed, and both mean a broken translation is
//! *invisible*: a dropped message silently serves English, and a dropped
//! `{ $count }` silently serves a sentence with the number missing.
//!
//! These tests close that gap. They are deliberately structural — they say
//! nothing about whether a translation is *good*, only that it is well-formed
//! and complete, which is the part a machine can decide.

use std::collections::{BTreeMap, BTreeSet};

use fluent_syntax::ast;
use fluent_syntax::parser::parse;

const SOURCE_LOCALE: &str = "en-US";

/// Every `$variable` referenced anywhere in a pattern, including inside select
/// variants and function-call arguments.
///
/// The recursion matters: `command-palette-result-count` hides its `$count`
/// inside a select expression, and the `NUMBER($v)` / `DATETIME($ts)` forms put
/// theirs inside a function's positional arguments. A shallow walk would call
/// those patterns variable-free and pass a translation that had lost them.
fn variables_in_pattern(pattern: &ast::Pattern<&str>, out: &mut BTreeSet<String>) {
    for element in &pattern.elements {
        if let ast::PatternElement::Placeable { expression } = element {
            variables_in_expression(expression, out);
        }
    }
}

fn variables_in_expression(expression: &ast::Expression<&str>, out: &mut BTreeSet<String>) {
    match expression {
        ast::Expression::Inline(inline) => variables_in_inline(inline, out),
        ast::Expression::Select { selector, variants } => {
            variables_in_inline(selector, out);
            for variant in variants {
                variables_in_pattern(&variant.value, out);
            }
        }
    }
}

fn variables_in_inline(inline: &ast::InlineExpression<&str>, out: &mut BTreeSet<String>) {
    match inline {
        ast::InlineExpression::VariableReference { id } => {
            out.insert(id.name.to_string());
        }
        ast::InlineExpression::FunctionReference { arguments, .. } => {
            for positional in &arguments.positional {
                variables_in_inline(positional, out);
            }
            for named in &arguments.named {
                variables_in_inline(&named.value, out);
            }
        }
        ast::InlineExpression::TermReference {
            arguments: Some(arguments),
            ..
        } => {
            for positional in &arguments.positional {
                variables_in_inline(positional, out);
            }
            for named in &arguments.named {
                variables_in_inline(&named.value, out);
            }
        }
        ast::InlineExpression::Placeable { expression } => {
            variables_in_expression(expression, out);
        }
        _ => {}
    }
}

/// Parse one locale into `message id -> the set of variables it interpolates`,
/// panicking with the parser's own diagnostics if the file is not valid Fluent.
fn parse_locale(tag: &str, source: &str) -> BTreeMap<String, BTreeSet<String>> {
    let resource = match parse(source) {
        Ok(resource) => resource,
        Err((_, errors)) => panic!(
            "{tag}.ftl is not valid Fluent: {} parse error(s): {errors:?}",
            errors.len()
        ),
    };

    let mut messages = BTreeMap::new();
    for entry in &resource.body {
        if let ast::Entry::Message(message) = entry {
            let mut variables = BTreeSet::new();
            if let Some(value) = &message.value {
                variables_in_pattern(value, &mut variables);
            }
            for attribute in &message.attributes {
                variables_in_pattern(&attribute.value, &mut variables);
            }
            if messages
                .insert(message.id.name.to_string(), variables)
                .is_some()
            {
                panic!("{tag}.ftl defines `{}` more than once", message.id.name);
            }
        }
    }
    messages
}

fn locales() -> Vec<(&'static str, String)> {
    teksilo_widgets::framework_locales()
        .iter()
        .map(|(tag, resources)| (*tag, resources.concat()))
        .collect()
}

fn source_messages() -> BTreeMap<String, BTreeSet<String>> {
    let locales = locales();
    let (tag, source) = locales
        .iter()
        .find(|(tag, _)| *tag == SOURCE_LOCALE)
        .unwrap_or_else(|| panic!("framework_locales() must contain the {SOURCE_LOCALE} source"));
    parse_locale(tag, source)
}

/// Every registered locale is valid Fluent.
///
/// A `.ftl` that fails to parse is not a loud failure at runtime: `fluent-bundle`
/// keeps the entries it managed to read and drops the rest, so a stray brace can
/// quietly delete half a translation.
#[test]
fn every_shipped_locale_parses_as_fluent() {
    for (tag, source) in locales() {
        let messages = parse_locale(tag, &source);
        assert!(
            !messages.is_empty(),
            "{tag}.ftl parsed but defines no messages"
        );
    }
}

/// Every translation defines exactly the messages the source does.
///
/// A *missing* key is survivable — `resolve_widget` falls back to en-US — but it
/// is still a translation gap nobody would otherwise see. An *extra* key is
/// worse: it is dead weight that no `tr_widget!` call site can ever reach,
/// usually the residue of a renamed key.
#[test]
fn every_locale_defines_the_same_keys_as_the_source() {
    let source = source_messages();
    let source_keys: BTreeSet<&String> = source.keys().collect();

    let mut failures = Vec::new();
    for (tag, text) in locales() {
        if tag == SOURCE_LOCALE {
            continue;
        }
        let messages = parse_locale(tag, &text);
        let keys: BTreeSet<&String> = messages.keys().collect();

        let missing: Vec<_> = source_keys.difference(&keys).collect();
        let extra: Vec<_> = keys.difference(&source_keys).collect();
        if !missing.is_empty() {
            failures.push(format!(
                "{tag}: {} key(s) missing vs {SOURCE_LOCALE}: {missing:?}",
                missing.len()
            ));
        }
        if !extra.is_empty() {
            failures.push(format!(
                "{tag}: {} key(s) not present in {SOURCE_LOCALE}: {extra:?}",
                extra.len()
            ));
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// Every translation interpolates exactly the variables the source does.
///
/// This is the invariant with teeth. Fluent resolves an unknown `$variable` to
/// the literal placeholder text and a *missing* one to nothing at all, and in
/// both cases the string still renders — so a translation that dropped
/// `{ $count }` from "Added { $count } files" produces the confidently wrong
/// "Added files" rather than any kind of error.
#[test]
fn every_locale_interpolates_the_same_variables_as_the_source() {
    let source = source_messages();

    let mut failures = Vec::new();
    for (tag, text) in locales() {
        if tag == SOURCE_LOCALE {
            continue;
        }
        let messages = parse_locale(tag, &text);
        for (key, expected) in &source {
            let Some(actual) = messages.get(key) else {
                continue; // reported by the key-parity test
            };
            if actual != expected {
                failures.push(format!(
                    "{tag}: `{key}` interpolates {actual:?}, source interpolates {expected:?}"
                ));
            }
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// Every `.ftl` on disk is registered, and every registration resolves.
///
/// `framework_locales()` is a hand-maintained list of `include_str!`s. An
/// unregistered file is inert — it sits in the tree looking like a shipped
/// translation while never reaching a single user — which is exactly the state
/// this whole test file was added to prevent recurring.
#[test]
fn every_locale_file_on_disk_is_registered() {
    let dir = concat!(env!("CARGO_MANIFEST_DIR"), "/locales");
    let mut on_disk: BTreeSet<String> = BTreeSet::new();
    for entry in std::fs::read_dir(dir).expect("locales/ directory") {
        let path = entry.expect("readable dir entry").path();
        if path.extension().is_some_and(|e| e == "ftl") {
            on_disk.insert(
                path.file_stem()
                    .expect("file stem")
                    .to_string_lossy()
                    .into_owned(),
            );
        }
    }

    let registered: BTreeSet<String> = teksilo_widgets::framework_locales()
        .iter()
        .map(|(tag, _)| (*tag).to_string())
        .collect();

    let unregistered: Vec<_> = on_disk.difference(&registered).collect();
    assert!(
        unregistered.is_empty(),
        "locales/ contains {} .ftl file(s) missing from framework_locales(), \
         so they ship in the repo but never load at runtime: {unregistered:?}",
        unregistered.len()
    );
}
