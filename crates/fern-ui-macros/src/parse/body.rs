//! Body parser: sequence of body items inside `{ ... }`.
//!
//! Dispatches by two-token lookahead per spec §3.1 "commit on distinctive
//! prefix":
//!
//! ```text
//! ident `:` <args>                  → property
//! lowercase-ident at EOL            → argument-free property
//! UpperCamel-ident `(` / `{` / `::` → child element
//! UpperCamel-ident alone            → child element (e.g. Spacer)
//! ```
//!
//! Bindings, `#{ }` escape, structural forms, spreads are not handled in
//! Phase 1 — they fall through to an error on the first unrecognized
//! token. Phase 2/3 will add those arms.

use syn::Token;
use syn::parse::{ParseStream, Result};

use crate::diag;
use crate::ir::BodyItem;

use super::{parse_element, parse_property_args};

pub(crate) fn parse_body(input: ParseStream) -> Result<Vec<BodyItem>> {
    let mut items = Vec::new();
    while !input.is_empty() {
        let item = parse_body_item(input)?;
        items.push(item);
    }
    Ok(items)
}

fn parse_body_item(input: ParseStream) -> Result<BodyItem> {
    // Primary dispatch is on the kind of the leading ident. If the body
    // doesn't start with an ident, we reject with a targeted error.
    if !input.peek(syn::Ident) {
        let span = input.span();
        return Err(diag::error(
            span,
            "expected a property name, child element, or structural form",
        ));
    }

    // `ident :` → property. This check is before the element-start
    // checks because a lowercase Rust path like `module::Widget`
    // wouldn't fit a property (no `:` after single ident), so the
    // distinction is mechanical.
    if input.peek2(Token![:]) {
        return parse_property(input).map(BodyItem::Property);
    }

    // Otherwise we have a bare element. Whether the ident is
    // UpperCamel (child element) or lowercase-bare (argument-free
    // property like `fills_stack`) depends on the first character.
    // For Phase 1 we only handle the UpperCamel child case; the bare
    // lowercase-ident-as-zero-arg-property case and bindings are Phase
    // 1 extensions.
    let ident: &proc_macro2::Ident = &input.fork().parse()?;
    let first = ident
        .to_string()
        .chars()
        .next()
        .unwrap_or('_');
    if first.is_ascii_uppercase() {
        let element = parse_element(input)?;
        return Ok(BodyItem::Child(element));
    }

    // Phase 1 extension (still in scope): bare lowercase ident at body
    // position is an argument-free property. `Expand { fills_stack }`
    // desugars to `.fills_stack()`.
    let property = super::property::parse_property_no_args(input)?;
    Ok(BodyItem::Property(property))
}

fn parse_property(input: ParseStream) -> Result<crate::ir::FernProperty> {
    let name: syn::Ident = input.parse()?;
    let _colon: Token![:] = input.parse()?;
    let args = parse_property_args(input)?;
    Ok(crate::ir::FernProperty { name, args })
}
