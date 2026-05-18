//! The `bati!` DSL proc-macro for Bastyde widget trees.
//!
//! See `docs/bati-language-spec-v3.md` for the surface language. This
//! crate implements a one-to-one syntactic transform from the DSL to
//! Bastyde V2 builder calls. The macro's only job is to remove syntactic
//! noise: every construct desugars to code the user could have written
//! by hand.
//!
//! # Crate path dispatch
//!
//! Apps depend on `bastyde` (the umbrella crate) and receive `bati!`
//! through `bastyde::prelude`. The emitted code references Bastyde types
//! through `::bastyde::core::...`. Internal workspace crates (any crate
//! whose name starts with `bastyde-`) cannot depend on `bastyde` (circular),
//! so the macro detects `CARGO_PKG_NAME` and emits `::bastyde_core::...`
//! paths instead. This mirrors the pattern in `bastyde-i18n-macros/src/lib.rs`.

use proc_macro::TokenStream;
use proc_macro2::TokenStream as TokenStream2;
use quote::quote;
use syn::{DeriveInput, parse_macro_input};

mod intent_kind;
mod lower;

/// Crate-root token stream used inside emitted code. Internal bastyde
/// workspace library crates (which depend on `bastyde-core` directly but
/// typically not on the `bastyde` umbrella crate) route through
/// `::bastyde_core`; everything else — including examples and external
/// applications that depend on the umbrella crate — routes through
/// `::bastyde::core`.
///
/// The detection is an explicit allowlist rather than `starts_with("bastyde-")`
/// because application crates commonly share the `bastyde-` prefix (e.g. a
/// downstream `bastyde-app` binary), and the `bastyde-app` binary in this
/// workspace itself is one such case. A bare prefix match silently routed
/// them through `::bastyde_core`, which isn't a direct dependency from an
/// external app and failed to resolve.
#[allow(dead_code)]
pub(crate) fn bastyde_core_root() -> TokenStream2 {
    let pkg = std::env::var("CARGO_PKG_NAME").unwrap_or_default();
    let is_internal = matches!(
        pkg.as_str(),
        "bastyde-core"
            | "bastyde-canvas"
            | "bastyde-tokens"
            | "bastyde-widgets"
            | "bastyde-data"
            | "bastyde-text"
            | "bastyde-i18n"
            | "bastyde-i18n-macros"
            | "bastyde-macros"
            | "bastyde-render"
            | "bastyde-platform"
            | "bastyde-resources"
    );
    if is_internal {
        quote!(::bastyde_core)
    } else {
        quote!(::bastyde::core)
    }
}

/// The `bati!` DSL entry point. Parses a block-structured widget-tree
/// description and expands to a sequence of Bastyde V2 builder calls.
///
/// Two forms:
///
/// ```ignore
/// bati!(ctx => <root-element>)    // inserts into the arena via ctx.add,
///                                  // returns a WidgetId
/// bati!(<root-element>)           // returns a widget value, suitable
///                                  // for passing to .child(...) etc.
/// ```
///
/// See `docs/bati-language-spec-v3.md` for the full surface language.
#[proc_macro]
pub fn bati(input: TokenStream) -> TokenStream {
    match bastyde_parse::parse_root(input.into()) {
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
/// `IntentKind` trait in `bastyde-core::intent`).
#[proc_macro_derive(IntentKind, attributes(name))]
pub fn derive_intent_kind(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);
    match intent_kind::derive_intent_kind(input) {
        Ok(ts) => ts.into(),
        Err(err) => err.to_compile_error().into(),
    }
}
