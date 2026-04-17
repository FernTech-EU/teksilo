//! Property-argument parser.
//!
//! Spec §3.4: "The argument list of a property terminates at the next
//! newline, unless the last token on the line is inside an open bracket,
//! in which case parsing continues until the brackets balance."
//!
//! Implementation: syn delegates bracket-balancing to `syn::Expr` parsing
//! (each Expr consumes whatever nested brackets it contains). Between
//! args, we use span line info to decide whether a comma continues the
//! arg list or belongs to the next body item.
//!
//! Specifically: after parsing an `Expr`, we peek for a `,`. If the
//! comma's span starts on the same line as the Expr's LAST token, we
//! consume it and parse another Expr. If the comma is on a later line,
//! or there is no comma, the arg list ends.

use syn::parse::{ParseStream, Result};
use syn::spanned::Spanned;
use syn::{Expr, Token};

use crate::ir::FernProperty;

/// Parse the argument list of a property (everything after `name:`).
///
/// Multi-arg continuation rule (spec §3.4): after each Expr, if the
/// next token is a `,` whose start line equals the START line of the
/// just-parsed Expr, consume it and parse another Expr. Otherwise
/// stop.
///
/// Phase 1 limitation: a multi-line expression value (struct literal,
/// multi-line closure) followed by a continuation comma is not
/// supported — `Span::end()` isn't populated reliably under
/// `span-locations` on stable Rust, so we can't tell whether the comma
/// is on the same line as the expression's LAST token. Workaround:
/// split into separate properties, or paren-wrap the multi-line value.
pub(crate) fn parse_property_args(input: ParseStream) -> Result<Vec<Expr>> {
    let first: Expr = input.parse()?;
    let mut args = vec![first];

    loop {
        if !input.peek(Token![,]) {
            break;
        }
        let comma_line = input.cursor().span().start().line;
        let last_expr_line = args
            .last()
            .expect("args is non-empty by construction")
            .span()
            .start()
            .line;
        if comma_line != last_expr_line {
            break;
        }
        let _comma: Token![,] = input.parse()?;
        let next: Expr = input.parse()?;
        args.push(next);
    }

    Ok(args)
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
