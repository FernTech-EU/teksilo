// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Parser for the `teksu!` DSL.
//!
//! Entry point is [`parse_root`]. The parser is hand-written recursive
//! descent over `syn::parse::ParseStream`. Positional arguments, property
//! values, and `rust { }` blocks delegate to `syn::Expr` / `syn::Block`
//! so Rust's own grammar handles nested brackets and expressions.

use proc_macro2::TokenStream as TokenStream2;
use syn::Token;
use syn::parse::{ParseStream, Parser, Result};

use crate::ir::TeksiRoot;

mod body;
mod cursor;
mod element;
mod property;
mod structural;

pub fn parse_root(tokens: TokenStream2) -> Result<TeksiRoot> {
    Parser::parse2(parse_root_impl, tokens)
}

fn parse_root_impl(input: ParseStream) -> Result<TeksiRoot> {
    // Disambiguate the two invocation forms by two-token lookahead:
    //   teksu!(ctx => <element>)   — ident followed by `=>`
    //   teksu!(<element>)          — anything else
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
            "expected end of teksu! input — the root element must be the only top-level element",
        ));
    }

    Ok(TeksiRoot { ctx, root })
}

// Re-export the submodule parse functions for sibling modules.
pub(crate) use body::parse_body;
#[allow(unused_imports)]
pub(crate) use cursor::{peek_binding, peek_element_start, peek_escape};
pub(crate) use element::parse_element;
pub(crate) use property::parse_property_args;
pub(crate) use structural::{parse_for, parse_if, parse_match, parse_spread, peek_spread};
