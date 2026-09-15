// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

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
//! not yet implemented and fall through to a targeted error.

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
        // Commas between body items are accepted as optional separators
        // (so `Stack { Leaf, Leaf }` and `Panel { padding: 8.0, color: RED }`
        // match Rust struct-literal expectations). Spec §3 preferred
        // newline separators; this is a usability relaxation.
        while input.peek(Token![,]) {
            let _comma: Token![,] = input.parse()?;
        }
    }
    Ok(items)
}

fn parse_body_item(input: ParseStream) -> Result<BodyItem> {
    // `#{ expr }` — body-position escape. A WidgetId expression that
    // attaches via `.child(...)` on the parent.
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

    // An expression start that is not an identifier but still cannot be
    // anything except a widget-valued Rust expression: a keyword-rooted path
    // (`self.row(x)`, `Self::header()`, `crate::ui::header()`, `super::row()`).
    // Route it to `.child(expr)` rather than to the catch-all error. The
    // structural keywords, `#{ }` and `..` were all handled above.
    //
    // The list is exactly the starts that cannot CONTINUE the previous item.
    // Body items are separated by whitespace, not punctuation, so any start
    // that Rust would read as a continuation of the expression before it does
    // not belong here, however plausible it looks on its own:
    //
    //   - `(`: `(a) (b)` is a call, so a parenthesised item swallows its
    //     neighbour as an argument. Write `child: (expr)`, where the argument
    //     list is delimited.
    //   - `*`: `a *b` is a multiplication, so `section("x")` followed by
    //     `*boxed` silently becomes one expression. `Box<dyn Widget>` now
    //     implements `Widget`, so the deref buys nothing anyway.
    //   - `&`: `a &b` is a bitwise and, AND no reference type implements
    //     `Widget`, so this form can only ever be a mistake.
    //
    // A keyword path is safe on both counts: no Rust expression continues into
    // `self`, `Self`, `crate` or `super`.
    if input.peek(Token![self])
        || input.peek(Token![Self])
        || input.peek(Token![crate])
        || input.peek(Token![super])
    {
        let expr: Expr = input.parse()?;
        let span = syn::spanned::Spanned::span(&expr);
        return Ok(BodyItem::ExprChild { expr, span });
    }

    if !input.peek(syn::Ident) {
        let span = input.span();
        return Err(diag::error(
            span,
            "expected a property name, child element, binding, or `#{ expr }` escape",
        ));
    }

    // `ident :` → property. `peek2(Token![::])` goes first because
    // syn's `peek2(Token![:])` also matches the first colon of `::`,
    // which would misroute `Widget::new(lit!(...))` as a property
    // named `Widget`. Paths fall through to the element branch.
    if input.peek2(Token![:]) && !input.peek2(Token![::]) {
        return parse_property(input).map(BodyItem::Property);
    }

    // UpperCamel-starting ident — child element. Lowercase-starting —
    // argument-free property (e.g. `fills_stack`).
    let ident: syn::Ident = input.fork().parse()?;
    if super::cursor::ident_starts_upper(&ident) {
        let element = parse_element(input)?;
        return Ok(BodyItem::Child(element));
    }

    // A bare lowercase ident ALONE is an argument-free property
    // (`fills_stack`). A lowercase ident that continues into a call, a
    // method chain, a path or an index is a Rust *expression* producing a
    // widget: the `section("Header")` / `row(x).bold()` component-call
    // shape. Routing it to `.child(expr)` is a pure extension - every one
    // of these was a parse error before - and it is what lets a helper
    // function be a bare child.
    if !is_bare_no_arg_property(input) {
        let expr: Expr = input.parse()?;
        let span = syn::spanned::Spanned::span(&expr);
        return Ok(BodyItem::ExprChild { expr, span });
    }

    let property = super::property::parse_property_no_args(input)?;
    Ok(BodyItem::Property(property))
}

/// True when the cursor sits on a lowercase ident that ends right there:
/// the argument-free property form. Anything that continues (`(`, `.`,
/// `::`, `[`, `?`) is an expression.
fn is_bare_no_arg_property(input: ParseStream) -> bool {
    let fork = input.fork();
    if fork.parse::<syn::Ident>().is_err() {
        return false;
    }
    fork.is_empty()
        || fork.peek(Token![,])
        || (fork.peek(syn::Ident) && !fork.peek(Token![as]))
        || fork.peek(Token![if])
        || fork.peek(Token![for])
        || fork.peek(Token![match])
        || fork.peek(Token![let])
        || fork.peek(Token![#])
}

fn parse_property(input: ParseStream) -> Result<crate::ir::TeksiProperty> {
    let name: syn::Ident = input.parse()?;
    let _colon: Token![:] = input.parse()?;
    let args = parse_property_args(input)?;
    Ok(crate::ir::TeksiProperty { name, args })
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
