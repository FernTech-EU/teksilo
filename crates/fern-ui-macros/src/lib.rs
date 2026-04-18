//! The `fern!` DSL proc-macro for FernUI widget trees.
//!
//! See `docs/fern-language-spec-v3.md` for the surface language. This
//! crate implements a one-to-one syntactic transform from the DSL to
//! FernUI V2 builder calls. The macro's only job is to remove syntactic
//! noise: every construct desugars to code the user could have written
//! by hand.
//!
//! # Crate path dispatch
//!
//! Apps depend on `fern-ui` (the umbrella crate) and receive `fern!`
//! through `fern_ui::prelude`. The emitted code references FernUI types
//! through `::fern_ui::core::...`. Internal workspace crates (any crate
//! whose name starts with `fern-`) cannot depend on `fern-ui` (circular),
//! so the macro detects `CARGO_PKG_NAME` and emits `::fern_core::...`
//! paths instead. This mirrors the pattern in `fern-i18n-macros/src/lib.rs`.

use proc_macro::TokenStream;
use proc_macro2::TokenStream as TokenStream2;
use quote::quote;
use syn::{parse_macro_input, DeriveInput};

mod diag;
mod intent_kind;
mod ir;
mod lower;
mod parse;

/// Crate-root token stream used inside emitted code. Internal fern-*
/// crates route through `::fern_core`; external apps route through
/// `::fern_ui::core`. Unused in Phase 1 (no `IntoFernChild` routing
/// yet); wired up in Phase 2 for `#{ expr }` escape.
#[allow(dead_code)]
pub(crate) fn fern_core_root() -> TokenStream2 {
    let pkg = std::env::var("CARGO_PKG_NAME").unwrap_or_default();
    if pkg.starts_with("fern-") {
        quote!(::fern_core)
    } else {
        quote!(::fern_ui::core)
    }
}

/// The `fern!` DSL entry point. Parses a block-structured widget-tree
/// description and expands to a sequence of FernUI V2 builder calls.
///
/// Two forms:
///
/// ```ignore
/// fern!(ctx => <root-element>)    // inserts into the arena via ctx.add,
///                                  // returns a WidgetId
/// fern!(<root-element>)           // returns a widget value, suitable
///                                  // for passing to .child(...) etc.
/// ```
///
/// See `docs/fern-language-spec-v3.md` for the full surface language.
#[proc_macro]
pub fn fern(input: TokenStream) -> TokenStream {
    match parse::parse_root(input.into()) {
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
/// `IntentKind` trait in `fern-core::intent`).
#[proc_macro_derive(IntentKind, attributes(name))]
pub fn derive_intent_kind(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);
    match intent_kind::derive_intent_kind(input) {
        Ok(ts) => ts.into(),
        Err(err) => err.to_compile_error().into(),
    }
}
