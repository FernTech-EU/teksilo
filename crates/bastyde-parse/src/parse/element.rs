// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Element parser: `Type[::ctor][(args)] [{ body }]`.

use proc_macro2::Span;
use syn::parse::{ParseStream, Result};
use syn::punctuated::Punctuated;
use syn::{Expr, Path, Token, token};

use crate::diag;
use crate::ir::{BatiElement, BodyItem};

use super::parse_body;

/// Parse a single element starting at the current cursor position.
///
/// Grammar:
/// ```text
/// element := type_path ( "(" positional_args ")" )? ( "{" body "}" )?
/// ```
///
/// The `type_path` includes any explicit `::ctor` suffix. We don't try
/// to split off the last segment as a separate "constructor" node in the
/// IR — the whole path is the callable, and lowering appends `::new`
/// only when no explicit constructor was named. Determining "explicit"
/// vs "implicit" is a snake_case / UpperCamel heuristic on the last
/// segment, matching Rust's naming convention.
pub(crate) fn parse_element(input: ParseStream) -> Result<BatiElement> {
    let type_path: Path = input.parse()?;
    let head_span: Span = type_path
        .segments
        .first()
        .map(|s| s.ident.span())
        .unwrap_or_else(Span::call_site);

    // A lowercase last segment is an explicit constructor name
    // (Button::new_literal, Padding::uniform). An UpperCamel last
    // segment is a type name and we'll synthesize ::new at lower time.
    let has_explicit_ctor = type_path
        .segments
        .last()
        .map(|s| {
            let name = s.ident.to_string();
            name.chars()
                .next()
                .is_some_and(|c| c.is_ascii_lowercase() || c == '_')
        })
        .unwrap_or(false);

    // Optional positional args in `(...)`.
    let mut args_close: Option<Span> = None;
    let args: Vec<Expr> = if input.peek(token::Paren) {
        let content;
        let paren = syn::parenthesized!(content in input);
        args_close = Some(paren.span.close());
        let punct: Punctuated<Expr, Token![,]> = Punctuated::parse_terminated(&content)?;
        punct.into_iter().collect()
    } else {
        Vec::new()
    };

    // Optional body in `{...}`.
    let mut body_close: Option<Span> = None;
    let body = if input.peek(token::Brace) {
        let content;
        let brace = syn::braced!(content in input);
        body_close = Some(brace.span.close());
        parse_body(&content)?
    } else {
        Vec::new()
    };

    // Category B bare-child pre-empt (spec §9.2). If the parent type's
    // last path segment names a known Category B widget and the body
    // contains a bare child element, emit a targeted hint pointing at
    // a likely slot name. Without this, the user sees the compiler's
    // generic "no method named `child`" message without knowing which
    // slot to use.
    let leaf_name = type_path.segments.last().map(|s| s.ident.to_string());
    if let Some(name) = leaf_name.as_deref()
        && diag::is_category_b_widget(name)
        && let Some(child) = body.iter().find_map(|item| match item {
            BodyItem::Child(c) => Some(c),
            _ => None,
        })
    {
        return Err(diag::category_b_bare_child(name, child.head_span));
    }

    Ok(BatiElement {
        type_path,
        has_explicit_ctor,
        args,
        body,
        head_span,
        args_close,
        body_close,
    })
}
