//! Body parser: sequence of body items inside `{ ... }`.
//!
//! Dispatches by two-token lookahead per spec §3.1 "commit on distinctive
//! prefix":
//!
//! ```text
//! `#{` expr `}`                     → body-position escape (adds a WidgetId child)
//! ident `=` <element-start>         → binding (hoisted at lowering time)
//! ident `:` <args>                  → property
//! UpperCamel-ident `(`/`{`/`::`/EOL → child element
//! lowercase-ident alone             → argument-free property
//! ```
//!
//! Structural forms (`if`, `for`, `match`, `let`, spread, `rust`) are
//! out of Phase 2 scope and fall through to a targeted error.

use syn::parse::{ParseStream, Result};
use syn::{Block, Expr, Local, Stmt, Token};

use crate::diag;
use crate::ir::{BodyItem, RustShape};

use super::{
    parse_element, parse_for, parse_if, parse_match, parse_property_args, parse_spread,
    peek_binding, peek_escape, peek_spread,
};

pub(crate) fn parse_body(input: ParseStream) -> Result<Vec<BodyItem>> {
    let mut items = Vec::new();
    while !input.is_empty() {
        let item = parse_body_item(input)?;
        items.push(item);
        // Stray `,` between body items — users coming from JSON or
        // Rust struct literals expect comma separators. Spec §9.2.
        if input.peek(Token![,]) {
            return Err(diag::comma_between_body_items(input.span()));
        }
    }
    Ok(items)
}

fn parse_body_item(input: ParseStream) -> Result<BodyItem> {
    // `#{ expr }` — body-position escape. A WidgetId expression that
    // attaches via `.add_child(...)` on the parent.
    if peek_escape(input) {
        let pound_span = input.span();
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
        return Ok(BodyItem::Escape {
            expr,
            span: pound_span,
        });
    }

    // `name = Element` — binding.
    if peek_binding(input) {
        let name: syn::Ident = input.parse()?;
        let _eq: Token![=] = input.parse()?;
        let element = parse_element(input)?;
        return Ok(BodyItem::Binding { name, element });
    }

    // `let pat = expr;` — spec §5.4.
    if input.peek(Token![let]) {
        let local = parse_let_local(input)?;
        return Ok(BodyItem::Let(local));
    }

    // Structural keywords — spec §5.1–§5.3.
    if input.peek(Token![if]) {
        return parse_if(input).map(BodyItem::If);
    }
    if input.peek(Token![match]) {
        return parse_match(input).map(BodyItem::Match);
    }
    if input.peek(Token![for]) {
        return parse_for(input).map(BodyItem::For);
    }

    // `..expr` spread — spec §5.5.
    if peek_spread(input) {
        let (expr, span) = parse_spread(input)?;
        return Ok(BodyItem::Spread { expr, span });
    }

    // `rust { ... }` — spec §5.6.
    if input.peek(syn::Ident) {
        let ahead: syn::Ident = input.fork().parse()?;
        if ahead == "rust" && input.peek2(syn::token::Brace) {
            return parse_rust_block(input);
        }
    }

    if !input.peek(syn::Ident) {
        let span = input.span();
        return Err(diag::error(
            span,
            "expected a property name, child element, binding, or `#{ expr }` escape",
        ));
    }

    // `ident :` → property.
    if input.peek2(Token![:]) {
        return parse_property(input).map(BodyItem::Property);
    }

    // UpperCamel-starting ident — child element. Lowercase-starting —
    // argument-free property (e.g. `fills_stack`).
    let ident: syn::Ident = input.fork().parse()?;
    if super::cursor::ident_starts_upper(&ident) {
        let element = parse_element(input)?;
        return Ok(BodyItem::Child(element));
    }

    let property = super::property::parse_property_no_args(input)?;
    Ok(BodyItem::Property(property))
}

fn parse_property(input: ParseStream) -> Result<crate::ir::FernProperty> {
    let name: syn::Ident = input.parse()?;
    let _colon: Token![:] = input.parse()?;
    let args = parse_property_args(input)?;
    Ok(crate::ir::FernProperty { name, args })
}

/// Parse a `let` local at body position. Rust's Local grammar covers
/// patterns, type annotations, initializers, else-branches, and the
/// trailing semicolon — we delegate to syn's Stmt parser and extract
/// the Local arm.
fn parse_let_local(input: ParseStream) -> Result<Local> {
    let stmt: Stmt = input.parse()?;
    match stmt {
        Stmt::Local(local) => Ok(local),
        other => Err(diag::error(
            syn::spanned::Spanned::span(&other),
            "expected a `let` binding at this body position",
        )),
    }
}

/// Parse a `rust { ... }` body item. The shape (expression vs side
/// effect) is determined by the last statement of the block: a
/// `Stmt::Expr` with no trailing semicolon is expression form, any
/// other shape (including an empty block) is side-effect form.
fn parse_rust_block(input: ParseStream) -> Result<BodyItem> {
    let ident: syn::Ident = input.parse()?;
    let span = ident.span();
    let block: Block = input.parse()?;
    let shape = classify_block_shape(&block);
    Ok(BodyItem::Rust { block, span, shape })
}

fn classify_block_shape(block: &Block) -> RustShape {
    match block.stmts.last() {
        Some(Stmt::Expr(_, None)) => RustShape::Expression,
        _ => RustShape::SideEffect,
    }
}
