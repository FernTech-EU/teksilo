// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Property-argument parser.
//!
//! Spec §3.4 + §3.3 + §6.1. A property argument is one of:
//!
//! - `#{ expr }` — escape, expects a `WidgetId`.
//! - `name = Element` — binding, hoists a `let` and routes slots to
//!   `.prop_id(name)`.
//! - `TypePath [(args)] [{ body }]` — a bati element value
//!   (`tab: "name", Card { ... }`).
//! - Otherwise, an arbitrary Rust expression.
//!
//! Dispatch follows spec §3.1 "commit on distinctive prefix": the
//! decision is made from the leading 1-2 tokens with no backtracking.
//!
//! Multi-arg continuation rule: after each arg, if the next token is a
//! `,` and the token AFTER the comma doesn't look like the start of a
//! new body item (`ident:`, structural keyword, spread, escape), the
//! comma continues the arg list. Otherwise the arg list terminates
//! and the comma stays in the stream for the body parser to handle.
//!
//! This replaces an earlier line-based rule that relied on
//! `proc-macro2`'s `span-locations` feature, which interacts poorly
//! with rust-analyzer's proc-macro server. Syntactic lookahead works
//! under both cargo and rust-analyzer uniformly.

use syn::parse::{ParseStream, Result};
use syn::{Expr, Token};

use crate::diag;
use crate::ir::{BatiProperty, PropArg};

use super::{parse_element, peek_binding, peek_element_start, peek_escape, peek_spread};

/// Parse the argument list of a property (everything after `name:`).
/// Never empty — property body form always has at least one argument.
pub(crate) fn parse_property_args(input: ParseStream) -> Result<Vec<PropArg>> {
    let first = parse_prop_arg(input)?;
    let mut args = vec![first];

    loop {
        if !input.peek(Token![,]) {
            break;
        }
        if comma_begins_new_body_item(input) {
            break;
        }
        let _comma: Token![,] = input.parse()?;
        let next = parse_prop_arg(input)?;
        args.push(next);
    }

    Ok(args)
}

/// Peek past a comma that sits at the current cursor and decide
/// whether what follows looks like a body item. If yes, the comma is
/// stray and the arg list should terminate so the body
/// parser can surface the "use newlines, not commas" diagnostic.
fn comma_begins_new_body_item(input: ParseStream) -> bool {
    let fork = input.fork();
    if fork.parse::<Token![,]>().is_err() {
        return false;
    }

    // Structural keywords unambiguously begin body items.
    if fork.peek(Token![if])
        || fork.peek(Token![for])
        || fork.peek(Token![match])
        || fork.peek(Token![let])
    {
        return true;
    }
    // `..expr` spread.
    if peek_spread(&fork) {
        return true;
    }
    // `#{ expr }` escape.
    if peek_escape(&fork) {
        return true;
    }
    // `ident :` property (but NOT `ident ::` path, which is an
    // element arg value).
    if fork.peek(syn::Ident) && fork.peek2(Token![:]) && !fork.peek2(Token![::]) {
        return true;
    }
    // `ident =` binding with an element on the right.
    if peek_binding(&fork) {
        return true;
    }
    // An UpperCamel-starting element after the comma is kept as an
    // arg continuation so the `tab: "x", Card { ... }` pattern
    // (spec §3.4 TabWidget example) works. A comma followed by a
    // would-be new child on the next line is still treated as
    // continuation — users end a property without a trailing comma to
    // begin a new body item.
    false
}

fn parse_prop_arg(input: ParseStream) -> Result<PropArg> {
    if peek_escape(input) {
        let _pound: Token![#] = input.parse()?;
        let content;
        let _brace = syn::braced!(content in input);
        let expr: Expr = content.parse()?;
        if !content.is_empty() {
            return Err(diag::error(
                content.span(),
                "expected a single expression inside `#{ ... }`",
            ));
        }
        return Ok(PropArg::Escape(expr));
    }

    if peek_binding(input) {
        let name: syn::Ident = input.parse()?;
        let _eq: Token![=] = input.parse()?;
        let element = parse_element(input)?;
        return Ok(PropArg::Binding { name, element });
    }

    if peek_element_start(input) {
        let element = parse_element(input)?;
        return Ok(PropArg::Element(element));
    }

    let expr: Expr = input.parse()?;
    Ok(PropArg::Expr(expr))
}

/// Parse an argument-free property: a bare lowercase ident that sits
/// alone at body position, e.g. `fills_stack` in `Expand { fills_stack }`.
pub(crate) fn parse_property_no_args(input: ParseStream) -> Result<BatiProperty> {
    let name: syn::Ident = input.parse()?;
    Ok(BatiProperty {
        name,
        args: Vec::new(),
    })
}
