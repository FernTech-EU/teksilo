//! Lowering: walk the IR and emit builder-call token streams.
//!
//! Two emission shapes:
//!
//! - Chain form: `TypePath::ctor(args).prop(v).child(c)…`. Default
//!   when the element body contains only properties, bare children,
//!   bindings, and body-position escapes.
//! - Statement-sequence form: `{ let mut __parent = …; __parent =
//!   __parent.prop(v); …; __parent }`. Used when the body contains a
//!   `let` binding (spec §5.4) or a side-effect `rust { … }` block
//!   (spec §5.6), because those forms introduce statements rather
//!   than a method-chain link.
//!
//! Bindings anywhere in the tree still hoist to the outermost fern!
//! block (spec §3.3). Statement-sequence elements do not shadow that
//! rule — they only change how the element's own body is woven.

use proc_macro2::TokenStream as TokenStream2;
use quote::{quote, quote_spanned};

use crate::ir::{BodyItem, FernElement, FernProperty, FernRoot, PropArg, RustShape};

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

fn lower_element(
    e: &FernElement,
    ctx_tok: &TokenStream2,
    hoisted: &mut Vec<TokenStream2>,
) -> TokenStream2 {
    if body_needs_stmt_form(&e.body) {
        lower_element_stmt(e, ctx_tok, hoisted)
    } else {
        lower_element_chain(e, ctx_tok, hoisted)
    }
}

fn body_needs_stmt_form(body: &[BodyItem]) -> bool {
    body.iter().any(|item| {
        matches!(
            item,
            BodyItem::Let(_) | BodyItem::Rust { shape: RustShape::SideEffect, .. }
        )
    })
}

/// Pure chain form: `Parent::new().prop().child()...` emitted in source
/// order.
fn lower_element_chain(
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
            BodyItem::Rust {
                block,
                span,
                shape: RustShape::Expression,
            } => {
                // Expression-form `rust { ... expr }` produces a widget
                // value used as a child.
                out = quote_spanned! { *span =>
                    #out.child(#block)
                };
            }
            BodyItem::Let(_) | BodyItem::Rust { shape: RustShape::SideEffect, .. } => {
                unreachable!(
                    "body_needs_stmt_form should have routed this to stmt form"
                );
            }
        }
    }
    out
}

/// Statement-sequence form: `{ let mut __parent = ...; __parent = ...; __parent }`.
fn lower_element_stmt(
    e: &FernElement,
    ctx_tok: &TokenStream2,
    hoisted: &mut Vec<TokenStream2>,
) -> TokenStream2 {
    let ctor = chain::lower_ctor_call(e);
    let mut stmts: Vec<TokenStream2> = Vec::new();
    stmts.push(quote! { let mut __parent = #ctor; });

    for item in &e.body {
        match item {
            BodyItem::Property(prop) => {
                let call = lower_property_call(prop, ctx_tok, hoisted);
                stmts.push(quote! { __parent = __parent #call; });
            }
            BodyItem::Child(child) => {
                let child_expr = lower_element(child, ctx_tok, hoisted);
                let dot_span = child.head_span;
                stmts.push(quote_spanned! { dot_span =>
                    __parent = __parent.child(#child_expr);
                });
            }
            BodyItem::Binding { name, element } => {
                let element_expr = lower_element(element, ctx_tok, hoisted);
                let name_span = name.span();
                hoisted.push(quote_spanned! { name_span =>
                    let #name = #ctx_tok.add(#element_expr);
                });
                stmts.push(quote_spanned! { name_span =>
                    __parent = __parent.add_child(#name);
                });
            }
            BodyItem::Escape { expr, span } => {
                stmts.push(quote_spanned! { *span =>
                    __parent = __parent.add_child(#expr);
                });
            }
            BodyItem::Let(local) => {
                stmts.push(quote! { #local });
            }
            BodyItem::Rust {
                block,
                span,
                shape: RustShape::Expression,
            } => {
                stmts.push(quote_spanned! { *span =>
                    __parent = __parent.child(#block);
                });
            }
            BodyItem::Rust {
                block,
                span,
                shape: RustShape::SideEffect,
            } => {
                stmts.push(quote_spanned! { *span => #block });
            }
        }
    }

    quote! {
        {
            #(#stmts)*
            __parent
        }
    }
}

/// Lower a property into the `.name(args...)` suffix used in both forms.
fn lower_property_call(
    prop: &FernProperty,
    ctx_tok: &TokenStream2,
    hoisted: &mut Vec<TokenStream2>,
) -> TokenStream2 {
    let name = &prop.name;
    let method_span = name.span();

    if prop.args.is_empty() {
        return quote_spanned! { method_span =>
            .#name()
        };
    }

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
            .#id_name(#(#lowered_args),*)
        }
    } else {
        quote_spanned! { method_span =>
            .#name(#(#lowered_args),*)
        }
    }
}

fn lower_property_on(
    prev: TokenStream2,
    prop: &FernProperty,
    ctx_tok: &TokenStream2,
    hoisted: &mut Vec<TokenStream2>,
) -> TokenStream2 {
    let call = lower_property_call(prop, ctx_tok, hoisted);
    quote! { #prev #call }
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
