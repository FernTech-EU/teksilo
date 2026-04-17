//! Lowering: walk the IR and emit builder-call token streams.
//!
//! Phase 2 covers chain form for the pure case plus a statement-sequence
//! wrapper when bindings or body-position escapes are present. Bindings
//! anywhere in the tree hoist to the outermost fern! block per spec §3.3
//! — until structural forms introduce additional statement-forming
//! blocks in Phase 3.

use proc_macro2::TokenStream as TokenStream2;
use quote::{quote, quote_spanned};

use crate::ir::{BodyItem, FernElement, FernProperty, FernRoot, PropArg};

mod chain;

pub(crate) fn lower_root(root: &FernRoot) -> TokenStream2 {
    let ctx_tok: TokenStream2 = match &root.ctx {
        Some(ident) => quote!(#ident),
        None => quote!(ctx),
    };

    let mut hoisted = Vec::new();
    let element_expr = lower_element(&root.root, &ctx_tok, &mut hoisted);

    let body = match &root.ctx {
        Some(ctx) => quote!(#ctx.add(#element_expr)),
        None => element_expr,
    };

    if hoisted.is_empty() {
        body
    } else {
        quote! {
            {
                #(#hoisted)*
                #body
            }
        }
    }
}

/// Lower an element to a chain-form builder expression:
///   `TypePath::ctor(args).prop(...).child(lowered_child)...`
///
/// Bindings encountered along the way push `let name = ctx.add(...);`
/// statements into `hoisted` and lower their use-site to
/// `.add_child(name)` / `.prop_id(name)`.
fn lower_element(
    e: &FernElement,
    ctx_tok: &TokenStream2,
    hoisted: &mut Vec<TokenStream2>,
) -> TokenStream2 {
    let mut out = chain::lower_ctor_call(e);
    for item in &e.body {
        match item {
            BodyItem::Property(prop) => {
                out = lower_property_on(out, prop, ctx_tok, hoisted);
            }
            BodyItem::Child(child) => {
                let child_expr = lower_element(child, ctx_tok, hoisted);
                let dot_span = child.head_span;
                out = quote_spanned! { dot_span =>
                    #out.child(#child_expr)
                };
            }
            BodyItem::Binding { name, element } => {
                let element_expr = lower_element(element, ctx_tok, hoisted);
                let name_span = name.span();
                hoisted.push(quote_spanned! { name_span =>
                    let #name = #ctx_tok.add(#element_expr);
                });
                out = quote_spanned! { name_span =>
                    #out.add_child(#name)
                };
            }
            BodyItem::Escape { expr, span } => {
                out = quote_spanned! { *span =>
                    #out.add_child(#expr)
                };
            }
        }
    }
    out
}

fn lower_property_on(
    prev: TokenStream2,
    prop: &FernProperty,
    ctx_tok: &TokenStream2,
    hoisted: &mut Vec<TokenStream2>,
) -> TokenStream2 {
    let name = &prop.name;
    let method_span = name.span();

    if prop.args.is_empty() {
        return quote_spanned! { method_span =>
            #prev.#name()
        };
    }

    // Any Escape or Binding arg forces the `_id` slot suffix per spec
    // §A.3. Slots are the only properties with the `*_id` twin, so
    // non-slot properties that receive an escape or binding will
    // surface a clean "no method named X_id" error on the emitted token.
    let forces_id_suffix = prop
        .args
        .iter()
        .any(|a| matches!(a, PropArg::Escape(_) | PropArg::Binding { .. }));

    let lowered_args: Vec<TokenStream2> = prop
        .args
        .iter()
        .map(|arg| lower_prop_arg(arg, ctx_tok, hoisted))
        .collect();

    if forces_id_suffix {
        let id_name = syn::Ident::new(&format!("{}_id", name), name.span());
        quote_spanned! { method_span =>
            #prev.#id_name(#(#lowered_args),*)
        }
    } else {
        quote_spanned! { method_span =>
            #prev.#name(#(#lowered_args),*)
        }
    }
}

fn lower_prop_arg(
    arg: &PropArg,
    ctx_tok: &TokenStream2,
    hoisted: &mut Vec<TokenStream2>,
) -> TokenStream2 {
    match arg {
        PropArg::Expr(e) => quote!(#e),
        PropArg::Element(el) => lower_element(el, ctx_tok, hoisted),
        PropArg::Escape(expr) => quote!(#expr),
        PropArg::Binding { name, element } => {
            let element_expr = lower_element(element, ctx_tok, hoisted);
            let name_span = name.span();
            hoisted.push(quote_spanned! { name_span =>
                let #name = #ctx_tok.add(#element_expr);
            });
            quote!(#name)
        }
    }
}
