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
use syn::{Expr, Token};

use crate::diag;
use crate::ir::BodyItem;

use super::{parse_element, parse_property_args, peek_binding, peek_escape};

pub(crate) fn parse_body(input: ParseStream) -> Result<Vec<BodyItem>> {
    let mut items = Vec::new();
    while !input.is_empty() {
        let item = parse_body_item(input)?;
        items.push(item);
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
