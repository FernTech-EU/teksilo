//! Parser for the `fern!` DSL.
//!
//! Entry point is [`parse_root`]. The parser is hand-written recursive
//! descent over `syn::parse::ParseStream`. Positional arguments, property
//! values, and `rust { }` blocks delegate to `syn::Expr` / `syn::Block`
//! so Rust's own grammar handles nested brackets and expressions.

use proc_macro2::TokenStream as TokenStream2;
use syn::Token;
use syn::parse::{ParseStream, Parser, Result};

use crate::ir::FernRoot;

mod body;
mod cursor;
mod element;
mod property;
mod structural;

pub fn parse_root(tokens: TokenStream2) -> Result<FernRoot> {
    Parser::parse2(parse_root_impl, tokens)
}

fn parse_root_impl(input: ParseStream) -> Result<FernRoot> {
    // Disambiguate the two invocation forms by two-token lookahead:
    //   fern!(ctx => <element>)   — ident followed by `=>`
    //   fern!(<element>)          — anything else
    let ctx = if input.peek(syn::Ident) && input.peek2(Token![=>]) {
        let ident: syn::Ident = input.parse()?;
        let _arrow: Token![=>] = input.parse()?;
        Some(ident)
    } else {
        None
    };

    let root = element::parse_element(input)?;

    if !input.is_empty() {
        return Err(input.error(
            "expected end of fern! input — the root element must be the only top-level element",
        ));
    }

    Ok(FernRoot { ctx, root })
}

// Re-export the submodule parse functions for sibling modules.
pub(crate) use body::parse_body;
#[allow(unused_imports)]
pub(crate) use cursor::{peek_binding, peek_element_start, peek_escape};
pub(crate) use element::parse_element;
pub(crate) use property::parse_property_args;
pub(crate) use structural::{parse_for, parse_if, parse_match, parse_spread, peek_spread};
