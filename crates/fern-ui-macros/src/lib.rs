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
use syn::{DeriveInput, parse_macro_input};

mod intent_kind;
mod lower;

/// Crate-root token stream used inside emitted code. Internal fern-ui
/// workspace library crates (which depend on `fern-core` directly but
/// typically not on the `fern-ui` umbrella crate) route through
/// `::fern_core`; everything else — including examples and external
/// applications that depend on the umbrella crate — routes through
/// `::fern_ui::core`.
///
/// The detection is an explicit allowlist rather than `starts_with("fern-")`
/// because application crates commonly share the `fern-` prefix (e.g. a
/// downstream `fern-app` binary), and the `fern-app` binary in this
/// workspace itself is one such case. A bare prefix match silently routed
/// them through `::fern_core`, which isn't a direct dependency from an
/// external app and failed to resolve.
#[allow(dead_code)]
pub(crate) fn fern_core_root() -> TokenStream2 {
    let pkg = std::env::var("CARGO_PKG_NAME").unwrap_or_default();
    let is_internal = matches!(
        pkg.as_str(),
        "fern-core"
            | "fern-canvas"
            | "fern-tokens"
            | "fern-widgets"
            | "fern-data"
            | "fern-text"
            | "fern-i18n"
            | "fern-i18n-macros"
            | "fern-ui-macros"
            | "fern-render"
            | "fern-platform"
            | "fern-resources"
    );
    if is_internal {
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
    match fern_parse::parse_root(input.into()) {
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
