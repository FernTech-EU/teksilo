// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Parsers for structural body items: `if` / `match` / `for` / `..spread`.
//!
//! Each form's body is a single bati element (spec §5.1–§5.3). Users
//! wanting multi-child bodies inside a structural branch wrap in a
//! container element (VStack, HStack, …).

use syn::parse::{ParseStream, Result};
use syn::{Expr, Pat, Token, token};

use crate::diag;
use crate::ir::{BatiElse, BatiFor, BatiIf, BatiMatch, BatiMatchArm};

use super::parse_element;

/// Parse an `if` chain: `if cond { Elem } [else if cond { Elem }]* [else { Elem }]?`.
pub(crate) fn parse_if(input: ParseStream) -> Result<BatiIf> {
    let if_token: Token![if] = input.parse()?;
    let span = if_token.span;
    parse_if_tail(input, span)
}

fn parse_if_tail(input: ParseStream, span: proc_macro2::Span) -> Result<BatiIf> {
    // Conditions like `if MyStruct { ... }` could otherwise be greedy
    // — `parse_without_eager_brace` stops at `{` so the following
    // brace delimits the if-body.
    let cond = Expr::parse_without_eager_brace(input)?;

    let then_content;
    let then_brace = syn::braced!(then_content in input);
    let then = parse_element(&then_content)?;
    if !then_content.is_empty() {
        return Err(diag::error(
            then_content.span(),
            "if-body must contain exactly one element — wrap multiple in a container like VStack",
        ));
    }
    let body_close = then_brace.span.close();

    let else_branch = if input.peek(Token![else]) {
        let _else_token: Token![else] = input.parse()?;
        if input.peek(Token![if]) {
            let next_if_token: Token![if] = input.parse()?;
            let next = parse_if_tail(input, next_if_token.span)?;
            Some(Box::new(BatiElse::ElseIf(next)))
        } else {
            let else_content;
            let else_brace = syn::braced!(else_content in input);
            let element = parse_element(&else_content)?;
            if !else_content.is_empty() {
                return Err(diag::error(
                    else_content.span(),
                    "else-body must contain exactly one element",
                ));
            }
            Some(Box::new(BatiElse::Element {
                element,
                body_close: else_brace.span.close(),
            }))
        }
    } else {
        None
    };

    Ok(BatiIf {
        cond,
        then,
        else_branch,
        span,
        body_close,
    })
}

pub(crate) fn parse_match(input: ParseStream) -> Result<BatiMatch> {
    let match_token: Token![match] = input.parse()?;
    let span = match_token.span;
    let scrutinee = Expr::parse_without_eager_brace(input)?;

    let content;
    let brace = syn::braced!(content in input);
    let mut arms = Vec::new();
    while !content.is_empty() {
        arms.push(parse_match_arm(&content)?);
    }

    Ok(BatiMatch {
        scrutinee,
        arms,
        span,
        body_close: brace.span.close(),
    })
}

fn parse_match_arm(input: ParseStream) -> Result<BatiMatchArm> {
    let pat = Pat::parse_multi_with_leading_vert(input)?;
    let guard = if input.peek(Token![if]) {
        let if_token: Token![if] = input.parse()?;
        let cond: Expr = input.parse()?;
        Some((if_token, cond))
    } else {
        None
    };
    let _fat_arrow: Token![=>] = input.parse()?;
    let element = parse_element(input)?;
    // Arms may be comma-separated; trailing comma optional.
    if input.peek(Token![,]) {
        let _comma: Token![,] = input.parse()?;
    }
    Ok(BatiMatchArm {
        pat,
        guard,
        element,
    })
}

pub(crate) fn parse_for(input: ParseStream) -> Result<BatiFor> {
    let for_token: Token![for] = input.parse()?;
    let span = for_token.span;
    let pat = Pat::parse_multi_with_leading_vert(input)?;
    let _in: Token![in] = input.parse()?;
    let iter = Expr::parse_without_eager_brace(input)?;

    let content;
    let brace = syn::braced!(content in input);

    let mut lets = Vec::new();
    while content.peek(Token![let]) {
        let stmt: syn::Stmt = content.parse()?;
        match stmt {
            syn::Stmt::Local(local) => lets.push(local),
            other => {
                return Err(diag::error(
                    syn::spanned::Spanned::span(&other),
                    "expected `let` binding in for-body",
                ));
            }
        }
    }

    let element = parse_element(&content)?;
    if !content.is_empty() {
        return Err(diag::error(
            content.span(),
            "for-body may contain `let` bindings followed by exactly one element",
        ));
    }

    Ok(BatiFor {
        pat,
        iter,
        lets,
        element,
        span,
        body_close: brace.span.close(),
    })
}

/// Parse `..expr` — a spread. We consume the `..` and parse the
/// expression greedily; the caller places us at a body position where
/// an expression is expected.
pub(crate) fn parse_spread(input: ParseStream) -> Result<(Expr, proc_macro2::Span)> {
    let dot_dot: Token![..] = input.parse()?;
    let span = dot_dot.spans[0];
    let expr: Expr = input.parse()?;
    Ok((expr, span))
}

/// Peek whether the cursor is at a `..expr` spread form.
pub(crate) fn peek_spread(input: ParseStream) -> bool {
    // `..` alone is Token![..]; `..=` is Token![..=]. We only take the
    // plain spread form here.
    input.peek(Token![..]) && !input.peek(Token![..=]) && !input.peek2(token::Brace)
}
