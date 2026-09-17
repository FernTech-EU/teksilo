// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Drift guard for the `teksu!` DSL's builder-method list.
//!
//! # The divergence this exists to break the build over
//!
//! `teksilo-macros`'s lowering pass reorders a `teksu!` body: every property
//! whose name is a `WidgetBuilder` method is moved to the **end** of the
//! emitted builder chain, because those methods return
//! `WidgetWithHandlers<T>`, which exposes none of the wrapped widget's own
//! setters. The predicate that decides which names get moved is
//! `teksilo_parse::diag::is_widget_builder_method` — a hand-written
//! `matches!` list.
//!
//! A `WidgetBuilder` method absent from that list is therefore *not* moved.
//! It stays where the user wrote it, and any `.child(..)` / `.spacing(..)`
//! after it is resolved against `WidgetWithHandlers<T>` instead of the widget.
//! What the user sees is not "you used a method the DSL does not know about";
//! it is a `no method named child found for struct WidgetWithHandlers` error
//! pointing at a `.child` they did not write, inside a macro expansion. The list
//! is the only place the DSL learns the trait's surface, and nothing else in
//! the workspace fails when it falls behind.
//!
//! # What the guard checks, and why the return type is the criterion
//!
//! Membership is decided by the **return type**, not by a naming convention:
//! a `WidgetBuilder` method returning `WidgetWithHandlers<Self>` is exactly a
//! method that performs the wrap, and the wrap is what breaks the chain.
//! A method returning something else is not merely outside the list, it is a
//! defect: the rest of the chain silently retargets at the other type, and
//! because a wrapper widget has its own `.child(..)` the result compiles and
//! builds a different tree. [`foreign_wrapper_returns`] pins that set empty.
//!
//! [`missing_from_predicate`] is the load-bearing direction. [`stale_in_predicate`]
//! catches the opposite drift — a name kept in the list after the method it
//! refers to was renamed or removed — and admits methods that exist only on
//! the `impl WidgetWithHandlers<W>` block, since those are reachable as the
//! second and later links of a chain.
//!
//! # Why a separate crate
//!
//! `teksilo-parse` must stay publishable: a test there would need a
//! dev-dependency on `teksilo-core`, which sits above it in the dependency
//! graph, and would have to read a source file that no published tarball
//! contains. So the check lives in a workspace-only crate that depends on
//! both sides and is published by neither.
//!
//! # Why `syn` and not a regular expression
//!
//! A guard that undercounts the trait reports a clean bill of health it has
//! not earned, so the parse has to be exact rather than approximate. A return
//! type is separated from the body by an optional `where` clause, generic
//! parameters contain `>` of their own, and doc comments and string literals
//! contain the word `fn` — each a way a pattern match over the text silently
//! drops a method rather than failing loudly. `syn::parse_file` is the parser
//! rustc's own front end agrees with, and it is already a dependency of the
//! DSL crates.

use quote::ToTokens;
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

/// Absolute path to `teksilo-core`'s `widget_builder.rs`, resolved from this
/// crate's manifest directory.
///
/// The path is workspace-relative by construction; see the module docs on why
/// this crate is not published.
pub fn widget_builder_source_path() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../teksilo-core/src/widget_builder.rs")
}

/// Parse `widget_builder.rs` and return the names of every `WidgetBuilder`
/// trait method whose return type is `WidgetWithHandlers<Self>`.
///
/// Matching is on the return path's **last segment**, so a method written
/// against a qualified path (`-> crate::widget_builder::WidgetWithHandlers<Self>`)
/// counts the same as the bare form.
///
/// # Panics
///
/// Panics if the file cannot be read or parsed, or if it declares no
/// `WidgetBuilder` trait — each of which means the guard has lost its subject
/// and must not report a clean bill of health.
pub fn widget_builder_wrapping_methods(source: &str) -> BTreeSet<String> {
    let file = syn::parse_file(source).expect("widget_builder.rs must parse as Rust");
    let trait_item = file
        .items
        .iter()
        .find_map(|item| match item {
            syn::Item::Trait(t) if t.ident == "WidgetBuilder" => Some(t),
            _ => None,
        })
        .expect("widget_builder.rs must declare `trait WidgetBuilder`");

    trait_item
        .items
        .iter()
        .filter_map(|item| match item {
            syn::TraitItem::Fn(f) if returns_widget_with_handlers(&f.sig.output) => {
                Some(f.sig.ident.to_string())
            }
            _ => None,
        })
        .collect()
}

/// Parse `widget_builder.rs` and return every `WidgetBuilder` method whose
/// return type is a **widget wrapper other than** `WidgetWithHandlers<Self>`.
///
/// This is the hazard the reorder rule cannot see. `is_widget_builder_method`
/// is keyed on `-> WidgetWithHandlers<Self>`, correctly, because that return is
/// what breaks a builder chain in a way the reorder can repair. A method
/// returning a *different* wrapper breaks the chain in a way it cannot: the
/// rest of the chain silently retargets at the new widget, and because that
/// widget usually has its own `.child(..)`, nothing fails to compile. The
/// author gets a different tree.
///
/// `dim_when_inactive` / `dim_when_inactive_default` were exactly that, and
/// `teksu!(ctx => VStack { dim_when_inactive: 0.7  A  B })` mounted
/// `DimWhenInactive > B`, losing the stack and `A`. They were removed from the
/// trait; every other wrapper widget in the framework (`Fade`, `Blur`,
/// `Scale`, `Collapse`, `DimWhenInactive`) is used as `Wrapper::new().child(w)`
/// and carries no trait method.
///
/// The set must stay empty. A `WidgetBuilder` method should return
/// `WidgetWithHandlers<Self>`, `Self`, or a non-widget query type.
///
/// # Panics
///
/// Panics if the file cannot be parsed.
pub fn foreign_wrapper_returns(source: &str) -> BTreeSet<String> {
    let file = syn::parse_file(source).expect("widget_builder.rs must parse as Rust");
    let trait_item = file
        .items
        .iter()
        .find_map(|item| match item {
            syn::Item::Trait(t) if t.ident == "WidgetBuilder" => Some(t),
            _ => None,
        })
        .expect("widget_builder.rs must declare `trait WidgetBuilder`");

    trait_item
        .items
        .iter()
        .filter_map(|item| match item {
            syn::TraitItem::Fn(f) => {
                let syn::ReturnType::Type(_, ty) = &f.sig.output else {
                    return None;
                };
                if returns_widget_with_handlers(&f.sig.output) {
                    return None;
                }
                let syn::Type::Path(path) = ty.as_ref() else {
                    return None;
                };
                let last = path.path.segments.last()?;
                let name = last.ident.to_string();
                // `Self`, `bool`, `Option<..>`, `Vec<WidgetId>` and friends are
                // queries and pass-throughs, not wrappers. A wrapper is an
                // UpperCamel concrete type that is neither of those.
                let is_query = matches!(
                    name.as_str(),
                    "Self" | "Option" | "Vec" | "bool" | "String" | "WidgetId"
                ) || name.chars().next().is_some_and(|c| c.is_lowercase());
                if is_query {
                    None
                } else {
                    Some(f.sig.ident.to_string())
                }
            }
            _ => None,
        })
        .collect()
}

/// Parse `widget_builder.rs` and return the names of every inherent method on
/// `impl WidgetWithHandlers<W>` that returns `Self`.
///
/// These are the second-and-later links of a builder chain. A method here that
/// is absent from the trait cannot be the *first* call on a bare widget, so it
/// is not part of [`missing_from_predicate`]'s subject; it is admitted by
/// [`stale_in_predicate`] so a legitimately chain-only name is not reported as
/// stale.
///
/// # Panics
///
/// Panics if the file cannot be parsed.
pub fn chained_wrapper_methods(source: &str) -> BTreeSet<String> {
    let file = syn::parse_file(source).expect("widget_builder.rs must parse as Rust");
    file.items
        .iter()
        .filter_map(|item| match item {
            syn::Item::Impl(i)
                if i.trait_.is_none() && impl_self_ty_is(i, "WidgetWithHandlers") =>
            {
                Some(i)
            }
            _ => None,
        })
        .flat_map(|i| i.items.iter())
        .filter_map(|item| match item {
            syn::ImplItem::Fn(f) if returns_self(&f.sig.output) => Some(f.sig.ident.to_string()),
            _ => None,
        })
        .collect()
}

/// Trait methods that wrap but are absent from `predicate` — the divergence
/// that produces the "no method named child" error described in the module
/// docs.
///
/// A non-empty result is a build failure, not a warning: every name in it is a
/// method a user can write in a `teksu!` body today and get a diagnostic about
/// a call they did not make.
pub fn missing_from_predicate(source: &str, predicate: impl Fn(&str) -> bool) -> BTreeSet<String> {
    widget_builder_wrapping_methods(source)
        .into_iter()
        .filter(|name| !predicate(name))
        .collect()
}

/// Names accepted by `predicate` that name no method on either the
/// `WidgetBuilder` trait or the `impl WidgetWithHandlers<W>` block.
///
/// `predicate` cannot be enumerated — it is a `matches!` over string literals —
/// so the caller supplies the list it was written from. A non-empty result
/// means the list outlived a rename or a removal: harmless at the call site
/// (nothing resolves either way) but it silently stops covering whatever the
/// method became.
pub fn stale_in_predicate<'a>(
    source: &str,
    listed: impl IntoIterator<Item = &'a str>,
) -> BTreeSet<String> {
    let mut real = widget_builder_wrapping_methods(source);
    real.extend(chained_wrapper_methods(source));
    listed
        .into_iter()
        .filter(|name| !real.contains(*name))
        .map(str::to_owned)
        .collect()
}

fn returns_self(output: &syn::ReturnType) -> bool {
    match output {
        syn::ReturnType::Default => false,
        syn::ReturnType::Type(_, ty) => type_last_segment_is(ty, "Self"),
    }
}

fn returns_widget_with_handlers(output: &syn::ReturnType) -> bool {
    match output {
        syn::ReturnType::Default => false,
        syn::ReturnType::Type(_, ty) => type_last_segment_is(ty, "WidgetWithHandlers"),
    }
}

fn type_last_segment_is(ty: &syn::Type, ident: &str) -> bool {
    matches!(ty, syn::Type::Path(p)
        if p.path.segments.last().is_some_and(|s| s.ident == ident))
}

fn impl_self_ty_is(item: &syn::ItemImpl, ident: &str) -> bool {
    type_last_segment_is(&item.self_ty, ident)
}

/// Absolute path to `teksilo-parse`'s `diag.rs`, resolved from this crate's
/// manifest directory.
pub fn diag_source_path() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../teksilo-parse/src/diag.rs")
}

/// Parse `diag.rs` and return every string literal inside
/// `is_widget_builder_method`'s body — i.e. the list the predicate was written
/// from.
///
/// The predicate is a `matches!` over literals and so cannot be enumerated at
/// run time. Reading them back out of the source is what keeps this guard from
/// needing a third copy of the list, which would be one more thing to drift.
///
/// # Panics
///
/// Panics if the file cannot be parsed or declares no
/// `is_widget_builder_method`, either of which means the guard has lost its
/// subject.
pub fn predicate_listed_names(diag_source: &str) -> BTreeSet<String> {
    let file = syn::parse_file(diag_source).expect("diag.rs must parse as Rust");
    let f = file
        .items
        .iter()
        .find_map(|item| match item {
            syn::Item::Fn(f) if f.sig.ident == "is_widget_builder_method" => Some(f),
            _ => None,
        })
        .expect("diag.rs must declare `fn is_widget_builder_method`");

    let mut names = BTreeSet::new();
    collect_string_literals(f.block.to_token_stream(), &mut names);
    names
}

fn collect_string_literals(stream: proc_macro2::TokenStream, out: &mut BTreeSet<String>) {
    for tree in stream {
        match tree {
            proc_macro2::TokenTree::Literal(lit) => {
                if let syn::Lit::Str(s) = syn::Lit::new(lit) {
                    out.insert(s.value());
                }
            }
            proc_macro2::TokenTree::Group(g) => collect_string_literals(g.stream(), out),
            _ => {}
        }
    }
}
