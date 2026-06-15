// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! `#[derive(IntentKind)]` — generates the typed DTO bridge between an
//! app's intent enum and the framework's runtime `Intent`.
//!
//! Each variant declares a `#[name = "..."]` attribute. The whole
//! variant — including any fields it carries — becomes the intent's
//! type-erased payload; handlers recover it via `from_intent`.
//!
//! ```ignore
//! #[derive(Debug, IntentKind)]
//! enum AppIntent {
//!     #[name = "app.save"]        Save,
//!     #[name = "app.open"]        Open(PathBuf),
//!     #[name = "app.add_item"]    AddItem { id: i64, dto: CreateItemDto },
//! }
//! ```
//!
//! Generates:
//!
//! ```text
//! impl IntentKind for AppIntent {
//!     fn into_intent(self) -> Intent {
//!         let name: &'static str = match &self {
//!             Self::Save           => "app.save",
//!             Self::Open(..)       => "app.open",
//!             Self::AddItem { .. } => "app.add_item",
//!         };
//!         Intent::with_payload(name, self)
//!     }
//!
//!     fn from_intent(i: &Intent) -> Option<&Self> {
//!         i.payload::<Self>()
//!     }
//! }
//! ```
//!
//! Any variant shape works — unit, tuple, struct — because the
//! derive never inspects the fields. The only requirement is that
//! the enum itself is `'static`; typically apps also derive `Debug`
//! and `Clone` on the enum for convenience, but neither is required
//! by `IntentKind` (the payload is stored by `Rc<dyn Any>`, and
//! `from_intent` returns a reference).

use proc_macro2::TokenStream as TokenStream2;
use quote::quote;
use syn::{Data, DeriveInput, Fields, Meta, Variant};

pub fn derive_intent_kind(input: DeriveInput) -> syn::Result<TokenStream2> {
    let enum_ident = &input.ident;
    let Data::Enum(data_enum) = &input.data else {
        return Err(syn::Error::new_spanned(
            &input,
            "IntentKind can only be derived for enums",
        ));
    };

    let intent_root = crate::bastyde_core_root();
    let mut arms: Vec<TokenStream2> = Vec::new();
    let mut seen_names: Vec<String> = Vec::new();

    for variant in &data_enum.variants {
        let name = extract_variant_name(variant)?;
        if seen_names.iter().any(|n| n == &name) {
            return Err(syn::Error::new_spanned(
                variant,
                format!("duplicate intent name: {}", name),
            ));
        }
        seen_names.push(name.clone());

        let ident = &variant.ident;
        // A match arm that matches the variant regardless of its
        // field shape. The derive never inspects the fields — the
        // whole variant becomes the payload.
        let pattern = match &variant.fields {
            Fields::Unit => quote!(Self::#ident),
            Fields::Unnamed(_) => quote!(Self::#ident(..)),
            Fields::Named(_) => quote!(Self::#ident { .. }),
        };
        arms.push(quote! {
            #pattern => #name,
        });
    }

    Ok(quote! {
        impl #intent_root::intent::IntentKind for #enum_ident {
            fn into_intent(self) -> #intent_root::intent::Intent {
                // Match on a reference so `self` stays owned for the
                // `with_payload` call that consumes it into the
                // type-erased payload.
                let name: &'static str = match &self {
                    #(#arms)*
                };
                #intent_root::intent::Intent::with_payload(name, self)
            }

            fn from_intent(intent: &#intent_root::intent::Intent) -> Option<&Self> {
                intent.payload::<Self>()
            }
        }
    })
}

/// Pull the `#[name = "..."]` literal off a variant, or error if missing.
fn extract_variant_name(variant: &Variant) -> syn::Result<String> {
    for attr in &variant.attrs {
        if !attr.path().is_ident("name") {
            continue;
        }
        if let Meta::NameValue(nv) = &attr.meta
            && let syn::Expr::Lit(syn::ExprLit {
                lit: syn::Lit::Str(s),
                ..
            }) = &nv.value
        {
            return Ok(s.value());
        }
        return Err(syn::Error::new_spanned(
            attr,
            r#"expected #[name = "..."] with a string literal"#,
        ));
    }
    Err(syn::Error::new_spanned(
        variant,
        r#"variant is missing #[name = "..."]"#,
    ))
}
