// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Parser and IR for the `teksu!` DSL.
//!
//! Extracted from `teksilo-macros` so non-proc-macro consumers (notably
//! `cargo teksilo-fmt`) can build on the same grammar without depending on
//! a `proc-macro = true` crate. The proc-macro crate `teksilo-macros`
//! depends on this crate and only owns the lowering step.
//!
//! Surface:
//!
//! - [`parse_root`] — entry point. Takes a `proc_macro2::TokenStream`
//!   and returns a [`TeksiRoot`].
//! - [`ir`] module — IR types ([`TeksiRoot`], [`TeksiElement`],
//!   [`BodyItem`], …). All fields are `pub` so consumers can walk and
//!   reformat the tree.
//! - [`diag`] module — shared diagnostic helpers and the
//!   `is_widget_builder_method` / `is_category_b_widget` predicates
//!   used by both lowering and pretty-printing.
//!
//! See `docs/teksu-language-spec-v3.md` for the surface language.

pub mod diag;
pub mod ir;
mod parse;

pub use ir::{
    BodyItem, PropArg, RustShape, TeksiElement, TeksiElse, TeksiFor, TeksiIf, TeksiMatch,
    TeksiMatchArm, TeksiProperty, TeksiRoot,
};
pub use parse::parse_root;
