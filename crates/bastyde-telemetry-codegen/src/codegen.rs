//! Token generation — converts a validated `Schema` into `proc_macro2::TokenStream`.

use proc_macro2::{Ident, Span, TokenStream};
use quote::quote;

use crate::manifest::{EventDef, PropDef, Schema};

pub fn generate(schema: &Schema, warnings: &[String]) -> TokenStream {
    let schema_version = schema.schema_version;

    // Emit each expired-event warning as a doc comment on a dummy const
    // (stable-Rust alternative to proc_macro::Diagnostic on stable).
    let warning_tokens: Vec<TokenStream> = warnings
        .iter()
        .map(|w| {
            quote! {
                compile_error!(#w);
            }
        })
        .collect();

    let mut output = quote! {
        #(#warning_tokens)*
        pub const SCHEMA_VERSION: u32 = #schema_version;
    };

    for event in &schema.events {
        output.extend(generate_event(event, schema_version));
    }

    output
}

fn generate_event(event: &EventDef, schema_version: u32) -> TokenStream {
    let mut tokens = TokenStream::new();

    // Generate enum types for enum props.
    for prop in &event.props {
        if prop.ty == "enum" {
            tokens.extend(generate_enum(event, prop));
        }
    }

    // Generate the emit_* function.
    tokens.extend(generate_emit_fn(event, schema_version));

    tokens
}

fn generate_enum(event: &EventDef, prop: &PropDef) -> TokenStream {
    let type_name = Ident::new(&enum_type_name(&event.name, &prop.name), Span::call_site());

    let variant_idents: Vec<Ident> = prop
        .values
        .iter()
        .map(|v| Ident::new(&to_camel_case(v), Span::call_site()))
        .collect();
    let variant_strs: Vec<&str> = prop.values.iter().map(String::as_str).collect();

    quote! {
        #[derive(Copy, Clone, Debug, PartialEq, Eq)]
        pub enum #type_name {
            #(#variant_idents,)*
        }
        impl #type_name {
            pub fn as_str(self) -> &'static str {
                match self {
                    #(Self::#variant_idents => #variant_strs,)*
                }
            }
        }
    }
}

fn generate_emit_fn(event: &EventDef, schema_version: u32) -> TokenStream {
    let fn_name = Ident::new(&emit_fn_name(&event.name), Span::call_site());
    let event_name_lit = &event.name;
    let category_path = category_tokens(&event.category);

    // Build parameter list and prop array entries.
    let mut params: Vec<TokenStream> = vec![
        quote! { reporter: &dyn ::bastyde_telemetry::UsageReporter },
        quote! { install_id: ::std::option::Option<&str> },
        quote! { session_id: &str },
    ];
    let mut prop_entries: Vec<TokenStream> = Vec::new();

    for prop in &event.props {
        let (param_ty, prop_value) = prop_tokens(event, prop);
        let param_name = Ident::new(&prop.name, Span::call_site());
        let key_lit = prop.name.as_str();

        params.push(quote! { #param_name: #param_ty });
        prop_entries.push(quote! {
            ::bastyde_telemetry::Prop {
                key: #key_lit,
                value: #prop_value,
            }
        });
    }

    // If no props, emit an empty array; otherwise emit the filled array.
    let props_expr = if prop_entries.is_empty() {
        quote! { let props: [::bastyde_telemetry::Prop<'_>; 0] = []; }
    } else {
        quote! { let props = [#(#prop_entries),*]; }
    };

    quote! {
        pub fn #fn_name(#(#params),*) {
            #props_expr
            reporter.record(&::bastyde_telemetry::Event {
                name: #event_name_lit,
                category: #category_path,
                timestamp: ::std::time::SystemTime::now(),
                install_id,
                session_id,
                schema_version: #schema_version,
                props: &props,
            });
        }
    }
}

/// Returns (parameter type tokens, PropValue constructor tokens) for a prop.
fn prop_tokens(event: &EventDef, prop: &PropDef) -> (TokenStream, TokenStream) {
    let param_name = Ident::new(&prop.name, Span::call_site());
    match prop.ty.as_str() {
        "dev_static" => (
            quote! { &'static str },
            quote! { ::bastyde_telemetry::PropValue::StaticStr(#param_name) },
        ),
        "bounded_str" => (
            quote! { &str },
            quote! { ::bastyde_telemetry::PropValue::BoundedStr(#param_name) },
        ),
        "u32" => (
            quote! { u32 },
            quote! { ::bastyde_telemetry::PropValue::U32(#param_name) },
        ),
        "i64" => (
            quote! { i64 },
            quote! { ::bastyde_telemetry::PropValue::I64(#param_name) },
        ),
        "bool" => (
            quote! { bool },
            quote! { ::bastyde_telemetry::PropValue::Bool(#param_name) },
        ),
        "f64_bucket" => (
            quote! { ::bastyde_telemetry::F64Bucket },
            quote! { ::bastyde_telemetry::PropValue::F64Bucket(#param_name) },
        ),
        "enum" => {
            let enum_ty = Ident::new(&enum_type_name(&event.name, &prop.name), Span::call_site());
            (
                quote! { #enum_ty },
                quote! { ::bastyde_telemetry::PropValue::Enum { variant: #param_name.as_str() } },
            )
        }
        _ => unreachable!("validated before codegen"),
    }
}

fn category_tokens(category: &str) -> TokenStream {
    match category {
        "intent" => quote! { ::bastyde_telemetry::EventCategory::Intent },
        "lifecycle" => quote! { ::bastyde_telemetry::EventCategory::Lifecycle },
        "navigation" => quote! { ::bastyde_telemetry::EventCategory::Navigation },
        "census" => quote! { ::bastyde_telemetry::EventCategory::Census },
        "custom" => quote! { ::bastyde_telemetry::EventCategory::Custom },
        _ => unreachable!("validated before codegen"),
    }
}

// ----- name helpers ----------------------------------------------------------

/// `"intent.dispatched"` → `"emit_intent_dispatched"`
pub fn emit_fn_name(event_name: &str) -> String {
    let s: String = event_name
        .chars()
        .map(|c| if c == '.' || c == '-' { '_' } else { c })
        .collect();
    format!("emit_{s}")
}

/// `("intent.dispatched", "source")` → `"IntentDispatchedSource"`
pub fn enum_type_name(event_name: &str, prop_name: &str) -> String {
    to_camel_case(event_name) + &to_camel_case(prop_name)
}

/// `"app_started"` → `"AppStarted"`, `"intent.dispatched"` → `"IntentDispatched"`
pub fn to_camel_case(s: &str) -> String {
    s.split(['.', '_', '-'])
        .filter(|p| !p.is_empty())
        .map(|part| {
            let mut chars = part.chars();
            match chars.next() {
                None => String::new(),
                Some(first) => first.to_uppercase().to_string() + chars.as_str(),
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn emit_fn_name_converts_dots() {
        assert_eq!(emit_fn_name("intent.dispatched"), "emit_intent_dispatched");
        assert_eq!(
            emit_fn_name("lifecycle.app_started"),
            "emit_lifecycle_app_started"
        );
    }

    #[test]
    fn enum_type_name_camel_case() {
        assert_eq!(
            enum_type_name("intent.dispatched", "source"),
            "IntentDispatchedSource"
        );
        assert_eq!(
            enum_type_name("lifecycle.app_started", "theme_kind"),
            "LifecycleAppStartedThemeKind"
        );
    }

    #[test]
    fn to_camel_case_handles_mixed() {
        assert_eq!(to_camel_case("app_started"), "AppStarted");
        assert_eq!(to_camel_case("intent.dispatched"), "IntentDispatched");
        assert_eq!(to_camel_case("shortcut"), "Shortcut");
    }
}
