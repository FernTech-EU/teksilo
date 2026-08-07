// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Lowering: walk the IR and emit builder-call token streams.
//!
//! Two emission shapes:
//!
//! - Chain form: `TypePath::ctor(args).prop(v).child(c)…`. Default
//!   when the element body contains only properties, bare children,
//!   bindings, body-position escapes, `if`/`match`/`for`, and
//!   expression-form `rust { … }` blocks.
//! - Statement-sequence form: `{ let mut __parent = …; __parent =
//!   __parent.prop(v); …; __parent }`. Used when the body contains a
//!   `let` binding (§5.4), a side-effect `rust { … }` block (§5.6),
//!   or a `..spread` item (§5.5) — all of which introduce statements
//!   rather than a method-chain link.
//!
//! Bindings anywhere in the tree hoist to the outermost teksu! block
//! per spec §3.3. Per-structural-arm hoist scoping is a known
//! limitation: bindings declared inside an `if`/`else`/`match`/`for`
//! body currently hoist to the outer block rather than the arm.
//! Widgets are then created unconditionally even when the arm doesn't
//! run; the parent still only attaches to the child when the arm is
//! taken, so this is a performance rather than correctness concern.

use proc_macro2::{Span, TokenStream as TokenStream2};
use quote::{quote, quote_spanned};

use teksilo_parse::diag;
use teksilo_parse::{
    BodyItem, PropArg, RustShape, TeksiElement, TeksiElse, TeksiFor, TeksiIf, TeksiMatch,
    TeksiProperty, TeksiRoot,
};

mod chain;

pub(crate) fn lower_root(root: &TeksiRoot) -> TokenStream2 {
    let ctx_tok: TokenStream2 = match &root.ctx {
        Some(ident) => quote!(#ident),
        None => quote!(ctx),
    };

    let mut hoisted = Vec::new();
    let element_expr = match lower_element(&root.root, &ctx_tok, &mut hoisted) {
        Ok(ts) => ts,
        Err(err) => return err.to_compile_error(),
    };

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
    e: &TeksiElement,
    ctx_tok: &TokenStream2,
    hoisted: &mut Vec<TokenStream2>,
) -> Result<TokenStream2, syn::Error> {
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
            BodyItem::Let(_)
                | BodyItem::Rust {
                    shape: RustShape::SideEffect,
                    ..
                }
                | BodyItem::Spread { .. }
        )
    })
}

fn lower_element_chain(
    e: &TeksiElement,
    ctx_tok: &TokenStream2,
    hoisted: &mut Vec<TokenStream2>,
) -> Result<TokenStream2, syn::Error> {
    let mut out = chain::lower_ctor_call(e);
    for item in reordered_body(&e.body) {
        let attach = lower_body_attach(item, ctx_tok, hoisted)?;
        out = quote! { #out #attach };
    }
    Ok(out)
}

/// Return an iterator over body items with `WidgetBuilder`-method
/// properties moved to the end. Those methods wrap the widget in
/// `WidgetWithHandlers<T>` which doesn't expose per-widget setters,
/// so any `.child(...)` / `.spacing(...)` / `.slot_name(...)` after
/// them fails to resolve. Pushing handler properties to the end lets
/// users mix them with children in any source order.
///
/// Within each partition (non-handler, handler), the original source
/// order is preserved.
fn reordered_body(body: &[BodyItem]) -> impl Iterator<Item = &BodyItem> {
    let (non_handlers, handlers): (Vec<_>, Vec<_>) = body.iter().partition(|item| {
        !matches!(item, BodyItem::Property(p) if diag::is_widget_builder_method(&p.name.to_string()))
    });
    non_handlers.into_iter().chain(handlers)
}

fn lower_element_stmt(
    e: &TeksiElement,
    ctx_tok: &TokenStream2,
    hoisted: &mut Vec<TokenStream2>,
) -> Result<TokenStream2, syn::Error> {
    let ctor = chain::lower_ctor_call(e);
    let mut stmts: Vec<TokenStream2> = Vec::new();
    stmts.push(quote! { let mut __parent = #ctor; });

    for item in reordered_body(&e.body) {
        match item {
            BodyItem::Let(local) => {
                stmts.push(quote! { #local });
            }
            BodyItem::Rust {
                block,
                span,
                shape: RustShape::SideEffect,
            } => {
                stmts.push(quote_spanned! { *span => #block });
            }
            BodyItem::Spread { expr, span } => {
                stmts.push(quote_spanned! { *span =>
                    for __spread_id in #expr {
                        __parent = __parent.add_child(__spread_id);
                    }
                });
            }
            other => {
                let attach = lower_body_attach(other, ctx_tok, hoisted)?;
                stmts.push(quote! { __parent = __parent #attach; });
            }
        }
    }

    Ok(quote! {
        {
            #(#stmts)*
            __parent
        }
    })
}

/// Emit the `.method(...)` suffix for one body item (the piece that
/// attaches onto a parent). Used by both chain and statement-sequence
/// lowering — the difference between the two is only whether the
/// suffix threads through a method chain or mutates a `__parent` local.
fn lower_body_attach(
    item: &BodyItem,
    ctx_tok: &TokenStream2,
    hoisted: &mut Vec<TokenStream2>,
) -> Result<TokenStream2, syn::Error> {
    match item {
        BodyItem::Property(prop) => Ok(lower_property_call(prop, ctx_tok, hoisted)?),
        BodyItem::Child(child) => {
            let child_expr = lower_element(child, ctx_tok, hoisted)?;
            let dot_span = child.head_span;
            Ok(quote_spanned! { dot_span =>
                .child(#child_expr)
            })
        }
        BodyItem::Binding { name, element } => {
            let element_expr = lower_element(element, ctx_tok, hoisted)?;
            let name_span = name.span();
            hoisted.push(quote_spanned! { name_span =>
                let #name = #ctx_tok.add(#element_expr);
            });
            Ok(quote_spanned! { name_span =>
                .add_child(#name)
            })
        }
        BodyItem::Escape { expr, span } => Ok(quote_spanned! { *span =>
            .add_child(#expr)
        }),
        BodyItem::Rust {
            block,
            span,
            shape: RustShape::Expression,
        } => Ok(quote_spanned! { *span =>
            .child(#block)
        }),
        BodyItem::If(teksilo_if) => lower_if_attach(teksilo_if, ctx_tok, hoisted),
        BodyItem::Match(teksilo_match) => lower_match_attach(teksilo_match, ctx_tok, hoisted),
        BodyItem::For(teksilo_for) => lower_for_attach(teksilo_for, ctx_tok, hoisted),
        BodyItem::Let(_)
        | BodyItem::Rust {
            shape: RustShape::SideEffect,
            ..
        }
        | BodyItem::Spread { .. } => {
            unreachable!("stmt-form items shouldn't reach lower_body_attach")
        }
    }
}

fn lower_property_call(
    prop: &TeksiProperty,
    ctx_tok: &TokenStream2,
    hoisted: &mut Vec<TokenStream2>,
) -> Result<TokenStream2, syn::Error> {
    let name = &prop.name;
    let method_span = name.span();

    if prop.args.is_empty() {
        return Ok(quote_spanned! { method_span =>
            .#name()
        });
    }

    let forces_id_suffix = prop
        .args
        .iter()
        .any(|a| matches!(a, PropArg::Escape(_) | PropArg::Binding { .. }));

    let lowered_args: Vec<TokenStream2> = prop
        .args
        .iter()
        .map(|arg| lower_prop_arg(arg, ctx_tok, hoisted))
        .collect::<Result<Vec<_>, _>>()?;

    if forces_id_suffix {
        let id_name = syn::Ident::new(&format!("{}_id", name), name.span());
        Ok(quote_spanned! { method_span =>
            .#id_name(#(#lowered_args),*)
        })
    } else {
        Ok(quote_spanned! { method_span =>
            .#name(#(#lowered_args),*)
        })
    }
}

fn lower_prop_arg(
    arg: &PropArg,
    ctx_tok: &TokenStream2,
    hoisted: &mut Vec<TokenStream2>,
) -> Result<TokenStream2, syn::Error> {
    match arg {
        PropArg::Expr(e) => {
            let unwrapped = strip_outer_parens(e);
            Ok(quote!(#unwrapped))
        }
        PropArg::Element(el) => lower_element(el, ctx_tok, hoisted),
        PropArg::Escape(expr) => {
            let unwrapped = strip_outer_parens(expr);
            Ok(quote!(#unwrapped))
        }
        PropArg::Binding { name, element } => {
            let element_expr = lower_element(element, ctx_tok, hoisted)?;
            let name_span = name.span();
            hoisted.push(quote_spanned! { name_span =>
                let #name = #ctx_tok.add(#element_expr);
            });
            Ok(quote!(#name))
        }
    }
}

/// Strip redundant outer parens before emitting the expression into a
/// function-argument position. Users wrap method chains in parens to
/// bypass the parser's element-start detection (`peek_element_start`
/// requires an Ident as the first token); once we're past the parser
/// and ready to splice the expression back as an arg, the parens are
/// unused and rustc's `unused_parens` lint flags them.
fn strip_outer_parens(expr: &syn::Expr) -> &syn::Expr {
    let mut current = expr;
    while let syn::Expr::Paren(paren) = current {
        current = &paren.expr;
    }
    current
}

// ---------------------------------------------------------------------------
// Structural form lowering
// ---------------------------------------------------------------------------

fn lower_if_attach(
    teksilo_if: &TeksiIf,
    ctx_tok: &TokenStream2,
    hoisted: &mut Vec<TokenStream2>,
) -> Result<TokenStream2, syn::Error> {
    let (arm_count, has_final_else) = count_if_arms(teksilo_if);

    if arm_count == 1 && !has_final_else {
        let cond = &teksilo_if.cond;
        let then_expr = lower_element(&teksilo_if.then, ctx_tok, hoisted)?;
        return Ok(quote_spanned! { teksilo_if.span =>
            .child_opt(if #cond { Some(#then_expr) } else { None })
        });
    }

    if !has_final_else {
        return Err(diag::error(
            teksilo_if.span,
            "multi-arm `if` requires a final `else` branch — add `else { ... }` or drop the \
             else-if arms",
        ));
    }

    if arm_count > 4 {
        return Err(diag::error(
            teksilo_if.span,
            "teksu! supports up to 4 if-chain arms; wrap deeper chains in `Box<dyn Widget>` or \
             split into a helper",
        ));
    }

    let branch_path = teksilo_branch_path(arm_count);
    let branch_expr =
        lower_if_chain_as_branch(teksilo_if, ctx_tok, hoisted, arm_count, 0, &branch_path)?;
    Ok(quote_spanned! { teksilo_if.span =>
        .child(#branch_expr)
    })
}

fn count_if_arms(if_expr: &TeksiIf) -> (usize, bool) {
    let mut count = 1;
    let mut has_final_else = false;
    let mut cursor = &if_expr.else_branch;
    while let Some(boxed) = cursor {
        match &**boxed {
            TeksiElse::ElseIf(nested) => {
                count += 1;
                cursor = &nested.else_branch;
            }
            TeksiElse::Element { .. } => {
                count += 1;
                has_final_else = true;
                break;
            }
        }
    }
    (count, has_final_else)
}

fn lower_if_chain_as_branch(
    teksilo_if: &TeksiIf,
    ctx_tok: &TokenStream2,
    hoisted: &mut Vec<TokenStream2>,
    arm_count: usize,
    arm_index: usize,
    branch_path: &TokenStream2,
) -> Result<TokenStream2, syn::Error> {
    let cond = &teksilo_if.cond;
    let variant = branch_variant(arm_count, arm_index);
    let then_expr = lower_element(&teksilo_if.then, ctx_tok, hoisted)?;
    let then_wrapped = quote! { #branch_path::#variant(#then_expr) };

    let else_tail = match teksilo_if.else_branch.as_deref() {
        Some(TeksiElse::ElseIf(nested)) => {
            let else_expr = lower_if_chain_as_branch(
                nested,
                ctx_tok,
                hoisted,
                arm_count,
                arm_index + 1,
                branch_path,
            )?;
            quote!(else #else_expr)
        }
        Some(TeksiElse::Element { element, .. }) => {
            let variant = branch_variant(arm_count, arm_index + 1);
            let element_expr = lower_element(element, ctx_tok, hoisted)?;
            quote! { else { #branch_path::#variant(#element_expr) } }
        }
        None => {
            return Err(diag::error(
                teksilo_if.span,
                "missing `else` branch in multi-arm if — lower_if_chain_as_branch is only \
                 reachable with a final else",
            ));
        }
    };

    Ok(quote_spanned! { teksilo_if.span =>
        if #cond { #then_wrapped } #else_tail
    })
}

fn lower_match_attach(
    teksilo_match: &TeksiMatch,
    ctx_tok: &TokenStream2,
    hoisted: &mut Vec<TokenStream2>,
) -> Result<TokenStream2, syn::Error> {
    let arm_count = teksilo_match.arms.len();
    if arm_count < 2 {
        return Err(diag::error(
            teksilo_match.span,
            "`match` at body position needs at least 2 arms",
        ));
    }
    if arm_count > 4 {
        return Err(diag::error(
            teksilo_match.span,
            "teksu! supports up to 4 match arms; wrap deeper dispatches in `Box<dyn Widget>` or \
             split into a helper",
        ));
    }

    let branch_path = teksilo_branch_path(arm_count);
    let scrutinee = &teksilo_match.scrutinee;

    let mut lowered_arms = Vec::new();
    for (idx, arm) in teksilo_match.arms.iter().enumerate() {
        let variant = branch_variant(arm_count, idx);
        let pat = &arm.pat;
        let elem_expr = lower_element(&arm.element, ctx_tok, hoisted)?;
        let wrapped = quote!(#branch_path::#variant(#elem_expr));
        let arm_ts = if let Some((if_token, cond)) = &arm.guard {
            quote!(#pat #if_token #cond => #wrapped,)
        } else {
            quote!(#pat => #wrapped,)
        };
        lowered_arms.push(arm_ts);
    }

    Ok(quote_spanned! { teksilo_match.span =>
        .child(match #scrutinee {
            #(#lowered_arms)*
        })
    })
}

fn lower_for_attach(
    teksilo_for: &TeksiFor,
    ctx_tok: &TokenStream2,
    hoisted: &mut Vec<TokenStream2>,
) -> Result<TokenStream2, syn::Error> {
    let pat = &teksilo_for.pat;
    let iter = &teksilo_for.iter;
    let lets = &teksilo_for.lets;
    let elem_expr = lower_element(&teksilo_for.element, ctx_tok, hoisted)?;
    Ok(quote_spanned! { teksilo_for.span =>
        .children((#iter).map(|#pat| {
            #(#lets)*
            #elem_expr
        }))
    })
}

fn teksilo_branch_path(arm_count: usize) -> TokenStream2 {
    let root = crate::teksilo_core_root();
    match arm_count {
        2 => quote!(#root::TeksiBranch),
        3 => quote!(#root::TeksiBranch3),
        4 => quote!(#root::TeksiBranch4),
        _ => unreachable!("teksilo_branch_path called with unsupported arm count {arm_count}"),
    }
}

fn branch_variant(arm_count: usize, arm_index: usize) -> syn::Ident {
    let name = match (arm_count, arm_index) {
        (2, 0) => "L",
        (2, 1) => "R",
        (3, 0) => "A",
        (3, 1) => "B",
        (3, 2) => "C",
        (4, 0) => "A",
        (4, 1) => "B",
        (4, 2) => "C",
        (4, 3) => "D",
        _ => unreachable!("branch_variant out of range: {arm_count}, {arm_index}"),
    };
    syn::Ident::new(name, Span::call_site())
}
