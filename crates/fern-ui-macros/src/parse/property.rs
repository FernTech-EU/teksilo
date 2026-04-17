//! Property-argument parser.
//!
//! Spec §3.4 + §3.3 + §6.1. A property argument is one of:
//!
//! - `#{ expr }` — escape, expects a `WidgetId`.
//! - `name = Element` — binding, hoists a `let` and routes slots to
//!   `.prop_id(name)`.
//! - `TypePath [(args)] [{ body }]` — a fern element value
//!   (`tab_literal: "name", Card { ... }`).
//! - Otherwise, an arbitrary Rust expression.
//!
//! Dispatch follows spec §3.1 "commit on distinctive prefix": the
//! decision is made from the leading 1-2 tokens with no backtracking.
//!
//! Multi-arg continuation rule: after each arg, if the next token is a
//! `,` on the same line as the arg's start, consume it and parse
//! another arg. Otherwise stop.
//!
//! Phase 1 limitation (still in effect): a multi-line expression value
//! followed by a continuation comma on the last line is not reliably
//! detected — `Span::end()` isn't populated under `span-locations` on
//! stable Rust.

use syn::parse::{ParseStream, Result};
use syn::spanned::Spanned;
use syn::{Expr, Token};

use crate::diag;
use crate::ir::{FernProperty, PropArg};

use super::{parse_element, peek_binding, peek_element_start, peek_escape};

/// Parse the argument list of a property (everything after `name:`).
/// Never empty — property body form always has at least one argument.
pub(crate) fn parse_property_args(input: ParseStream) -> Result<Vec<PropArg>> {
    let first = parse_prop_arg(input)?;
    let mut args = vec![first];

    loop {
        if !input.peek(Token![,]) {
            break;
        }
        let comma_line = input.cursor().span().start().line;
        let last_arg_line = arg_span_start_line(
            args.last().expect("args is non-empty by construction"),
        );
        if comma_line != last_arg_line {
            break;
        }
        let _comma: Token![,] = input.parse()?;
        let next = parse_prop_arg(input)?;
        args.push(next);
    }

    Ok(args)
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

fn arg_span_start_line(arg: &PropArg) -> usize {
    match arg {
        PropArg::Expr(e) => e.span().start().line,
        PropArg::Element(e) => e.head_span.start().line,
        PropArg::Escape(e) => e.span().start().line,
        PropArg::Binding { name, .. } => name.span().start().line,
    }
}

/// Parse an argument-free property: a bare lowercase ident that sits
/// alone at body position, e.g. `fills_stack` in `Expand { fills_stack }`.
pub(crate) fn parse_property_no_args(input: ParseStream) -> Result<FernProperty> {
    let name: syn::Ident = input.parse()?;
    Ok(FernProperty {
        name,
        args: Vec::new(),
    })
}
