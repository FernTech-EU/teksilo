//! Parser and IR for the `fern!` DSL.
//!
//! Extracted from `fern-ui-macros` so non-proc-macro consumers (notably
//! `cargo fern-fmt`) can build on the same grammar without depending on
//! a `proc-macro = true` crate. The proc-macro crate `fern-ui-macros`
//! depends on this crate and only owns the lowering step.
//!
//! Surface:
//!
//! - [`parse_root`] — entry point. Takes a `proc_macro2::TokenStream`
//!   and returns a [`FernRoot`].
//! - [`ir`] module — IR types ([`FernRoot`], [`FernElement`],
//!   [`BodyItem`], …). All fields are `pub` so consumers can walk and
//!   reformat the tree.
//! - [`diag`] module — shared diagnostic helpers and the
//!   `is_widget_builder_method` / `is_category_b_widget` predicates
//!   used by both lowering and pretty-printing.
//!
//! See `docs/fern-language-spec-v3.md` for the surface language.

pub mod diag;
pub mod ir;
mod parse;

pub use ir::{
    BodyItem, FernElement, FernElse, FernFor, FernIf, FernMatch, FernMatchArm, FernProperty,
    FernRoot, PropArg, RustShape,
};
pub use parse::parse_root;
