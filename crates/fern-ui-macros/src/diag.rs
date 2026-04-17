//! Diagnostic helpers for the `fern!` macro.
//!
//! Every `compile_error!` emitted by expansion runs through these helpers
//! so the error span lands on a user token per spec §9.1. For Phase 1 the
//! helpers are thin wrappers over `syn::Error`; Phase 4 adds the
//! hardcoded Category-B hint table and the specific-message paths from
//! spec §9.2.

use proc_macro2::Span;
use syn::Error;

pub(crate) fn error<T: std::fmt::Display>(span: Span, msg: T) -> Error {
    Error::new(span, msg)
}
