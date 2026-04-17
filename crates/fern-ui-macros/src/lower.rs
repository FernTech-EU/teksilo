//! Lowering: walk the IR and emit builder-call token streams.
//!
//! Phase 1 covers the chain form only: elements, properties, and
//! bare-element children. Statement-sequence form (bindings, `let` at
//! body position, side-effect `rust { }` blocks, spread) is Phase 2+.

use proc_macro2::TokenStream as TokenStream2;
use quote::{quote, quote_spanned};

use crate::ir::{BodyItem, FernElement, FernProperty, FernRoot};

mod chain;

pub(crate) fn lower_root(root: &FernRoot) -> TokenStream2 {
    let element_expr = lower_element(&root.root);
    match &root.ctx {
        Some(ctx) => quote! { #ctx.add(#element_expr) },
        None => element_expr,
    }
}

/// Lower an element to a chain-form builder expression:
///   `TypePath::ctor(args).prop(...).child(lowered_child)...`
pub(crate) fn lower_element(e: &FernElement) -> TokenStream2 {
    let ctor_call = chain::lower_ctor_call(e);

    let mut out = ctor_call;
    for item in &e.body {
        match item {
            BodyItem::Property(prop) => {
                out = lower_property_on(out, prop);
            }
            BodyItem::Child(child) => {
                let child_expr = lower_element(child);
                let dot_span = child.head_span;
                out = quote_spanned! { dot_span =>
                    #out.child(#child_expr)
                };
            }
        }
    }
    out
}

fn lower_property_on(prev: TokenStream2, prop: &FernProperty) -> TokenStream2 {
    let name = &prop.name;
    let args = &prop.args;
    // Span the emitted method name to the user's property-name ident so
    // method-resolution errors land on the user's token (spec §9.1).
    let method_span = name.span();
    if args.is_empty() {
        quote_spanned! { method_span =>
            #prev.#name()
        }
    } else {
        quote_spanned! { method_span =>
            #prev.#name(#(#args),*)
        }
    }
}
