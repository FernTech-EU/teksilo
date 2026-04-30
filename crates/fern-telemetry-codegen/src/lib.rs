//! `fern-telemetry-codegen` — compile-time codegen for FernUI telemetry.
//!
//! Consumes a YAML event manifest at build time and expands to:
//!
//! - A `SCHEMA_VERSION: u32` constant.
//! - For each event with `enum`-typed props: a dedicated enum type with
//!   an `as_str()` method.
//! - For each event: a typed `emit_<event_name>(reporter, install_id,
//!   session_id, …props…)` free function that assembles an
//!   [`fern_telemetry::Event`] and hands it to the reporter.
//!
//! # Usage
//!
//! ```rust,ignore
//! // In your app's lib.rs or main.rs:
//! fern_telemetry_codegen::include_telemetry_schema!("telemetry/events.yaml");
//! ```
//!
//! The path is resolved relative to `CARGO_MANIFEST_DIR` (the crate
//! containing the macro invocation).
//!
//! # YAML format
//!
//! ```yaml
//! schema_version: 1
//! events:
//!   - name: intent.dispatched
//!     category: intent
//!     expires: "2027-06-01"
//!     bug: "https://github.com/your-org/your-app/issues/42"
//!     description: "Fired whenever an intent passes through dispatch."
//!     props:
//!       - name: name
//!         type: dev_static
//!       - name: source
//!         type: enum
//!         values: [shortcut, menu, handler, programmatic, accessibility]
//! ```
//!
//! # Property types
//!
//! | YAML type     | Rust type                       | `PropValue` variant         |
//! |---------------|---------------------------------|-----------------------------|
//! | `dev_static`  | `&'static str`                  | `StaticStr`                 |
//! | `bounded_str` | `&str`                          | `BoundedStr`                |
//! | `u32`         | `u32`                           | `U32`                       |
//! | `i64`         | `i64`                           | `I64`                       |
//! | `bool`        | `bool`                          | `Bool`                      |
//! | `f64_bucket`  | `fern_telemetry::F64Bucket`     | `F64Bucket`                 |
//! | `enum`        | generated enum (see above)      | `Enum { variant: &'static str }` |

use proc_macro::TokenStream;
use proc_macro2::Span;
use quote::quote;
use syn::{LitStr, parse_macro_input};

mod codegen;
mod manifest;
mod validation;

/// Expand a YAML telemetry manifest into typed `emit_*` functions.
///
/// See the [crate-level documentation](self) for the full reference.
#[proc_macro]
pub fn include_telemetry_schema(input: TokenStream) -> TokenStream {
    let path_lit = parse_macro_input!(input as LitStr);
    let relative_path = path_lit.value();

    // Resolve against the invoking crate's manifest directory.
    let manifest_dir = match std::env::var("CARGO_MANIFEST_DIR") {
        Ok(d) => d,
        Err(_) => {
            return error(Span::call_site(), "CARGO_MANIFEST_DIR is not set").into();
        }
    };
    let full_path =
        std::path::PathBuf::from(&manifest_dir).join(&relative_path);

    // Read the file.
    let content = match std::fs::read_to_string(&full_path) {
        Ok(s) => s,
        Err(e) => {
            return error(
                Span::call_site(),
                &format!("cannot read `{}`: {e}", full_path.display()),
            )
            .into();
        }
    };

    // Parse YAML.
    let schema = match manifest::parse_schema(&content) {
        Ok(s) => s,
        Err(e) => return error(Span::call_site(), &e).into(),
    };

    // Validate.
    let warnings = match validation::validate(&schema) {
        Ok(w) => w,
        Err(e) => return error(Span::call_site(), &e).into(),
    };

    // Generate typed emit_* functions.
    let generated = codegen::generate(&schema, &warnings);

    // Emit an include_str! reference so Cargo reruns when the YAML
    // file changes.
    let abs_path = full_path.to_string_lossy().to_string();
    let abs_path_lit = LitStr::new(&abs_path, Span::call_site());
    let file_tracker = quote! {
        const _TELEMETRY_SCHEMA_TRACKED: &str = ::std::include_str!(#abs_path_lit);
    };

    let out = quote! {
        #file_tracker
        #generated
    };
    out.into()
}

fn error(span: Span, message: &str) -> proc_macro2::TokenStream {
    syn::Error::new(span, message).to_compile_error()
}
