// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! The `teksu!` DSL proc-macro for Teksilo widget trees.
//!
//! See `docs/teksu-language-spec-v3.md` for the surface language. This
//! crate implements a one-to-one syntactic transform from the DSL to
//! Teksilo V2 builder calls. The macro's only job is to remove syntactic
//! noise: every construct desugars to code the user could have written
//! by hand.
//!
//! # Crate path dispatch
//!
//! Apps depend on `teksilo` (the umbrella crate) and receive `teksu!`
//! through `teksilo::prelude`. The emitted code references Teksilo types
//! through `::teksilo::core::...`. Internal workspace crates (any crate
//! whose name starts with `teksilo-`) cannot depend on `teksilo` (circular),
//! so the macro detects `CARGO_PKG_NAME` and emits `::teksilo_core::...`
//! paths instead. This mirrors the pattern in `teksilo-i18n-macros/src/lib.rs`.

use proc_macro::TokenStream;
use proc_macro2::TokenStream as TokenStream2;
use quote::quote;
use syn::{DeriveInput, parse_macro_input};

mod intent_kind;
mod lower;

/// Crate-root token stream used inside emitted code. Internal teksilo
/// workspace library crates (which depend on `teksilo-core` directly but
/// typically not on the `teksilo` umbrella crate) route through
/// `::teksilo_core`; everything else — including examples and external
/// applications that depend on the umbrella crate — routes through
/// `::teksilo::core`.
///
/// The detection is an explicit allowlist rather than `starts_with("teksilo-")`
/// because application crates commonly share the `teksilo-` prefix (e.g. a
/// downstream `teksilo-app` binary), and the `teksilo-app` binary in this
/// workspace itself is one such case. A bare prefix match silently routed
/// them through `::teksilo_core`, which isn't a direct dependency from an
/// external app and failed to resolve.
#[allow(dead_code)]
pub(crate) fn teksilo_core_root() -> TokenStream2 {
    let pkg = std::env::var("CARGO_PKG_NAME").unwrap_or_default();
    let is_internal = matches!(
        pkg.as_str(),
        "teksilo-core"
            | "teksilo-canvas"
            | "teksilo-tokens"
            | "teksilo-widgets"
            | "teksilo-data"
            | "teksilo-text"
            | "teksilo-i18n"
            | "teksilo-i18n-macros"
            | "teksilo-macros"
            | "teksilo-render"
            | "teksilo-platform"
            | "teksilo-resources"
    );
    if is_internal {
        quote!(::teksilo_core)
    } else {
        quote!(::teksilo::core)
    }
}

/// The `teksu!` DSL entry point. Parses a block-structured widget-tree
/// description and expands to a sequence of Teksilo V2 builder calls.
///
/// Two forms:
///
/// ```text
/// teksu!(ctx => <root-element>)    // inserts into the arena via ctx.add,
///                                  // returns a WidgetId
/// teksu!(<root-element>)           // returns a widget value, suitable
///                                  // for passing to .child(...) etc.
/// ```
///
/// See `docs/teksu-language-spec-v3.md` for the full surface language.
#[proc_macro]
pub fn teksu(input: TokenStream) -> TokenStream {
    match teksilo_parse::parse_root(input.into()) {
        Ok(root) => lower::lower_root(&root).into(),
        Err(err) => err.to_compile_error().into(),
    }
}

/// `#[derive(IntentKind)]` — generate a typed DTO bridge between an
/// app's intent enum and the runtime `Intent` dispatch type.
///
/// Each variant must carry a `#[name = "..."]` attribute; the string
/// is used as the runtime intent name. Unit variants are encoded as
/// parameter-less intents; tuple variants up to 4 fields encode into
/// `IntentParams::p1..p4` (primitives only — see the docs on the
/// `IntentKind` trait in `teksilo-core::intent`).
#[proc_macro_derive(IntentKind, attributes(name))]
pub fn derive_intent_kind(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);
    match intent_kind::derive_intent_kind(input) {
        Ok(ts) => ts.into(),
        Err(err) => err.to_compile_error().into(),
    }
}
