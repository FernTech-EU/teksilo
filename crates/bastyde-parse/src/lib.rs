//! Parser and IR for the `bati!` DSL.
//!
//! Extracted from `bastyde-macros` so non-proc-macro consumers (notably
//! `cargo bastyde-fmt`) can build on the same grammar without depending on
//! a `proc-macro = true` crate. The proc-macro crate `bastyde-macros`
//! depends on this crate and only owns the lowering step.
//!
//! Surface:
//!
//! - [`parse_root`] — entry point. Takes a `proc_macro2::TokenStream`
//!   and returns a [`BatiRoot`].
//! - [`ir`] module — IR types ([`BatiRoot`], [`BatiElement`],
//!   [`BodyItem`], …). All fields are `pub` so consumers can walk and
//!   reformat the tree.
//! - [`diag`] module — shared diagnostic helpers and the
//!   `is_widget_builder_method` / `is_category_b_widget` predicates
//!   used by both lowering and pretty-printing.
//!
//! See `docs/bati-language-spec-v3.md` for the surface language.

pub mod diag;
pub mod ir;
mod parse;

pub use ir::{
    BatiElement, BatiElse, BatiFor, BatiIf, BatiMatch, BatiMatchArm, BatiProperty, BatiRoot,
    BodyItem, PropArg, RustShape,
};
pub use parse::parse_root;
