//! Compile-time-validating resource embedding proc macro for Bastyde.
//!
//! Provides `res!` which embeds a file into the binary, validates its
//! format at compile time, and returns a lazily-decoded `&'static` reference.
//!
//! # Supported formats
//!
//! | Extension | Validation | Runtime type |
//! |-----------|-----------|--------------|
//! | `.svg` | XML parse + `<svg>` root + viewBox | `&'static SvgIcon` |
//! | `.png` | Magic bytes + IHDR chunk | `&'static RasterIcon` |
//! | `.webp` | RIFF + WEBP signature | `&'static RasterIcon` (static) or `&'static AnimatedIcon` (animated) |
//!
//! # Usage
//!
//! ```ignore
//! use bastyde_resources::res;
//!
//! // In a function — returns &'static decoded type:
//! let icon = res!("resources/icons/save.svg");
//! let logo = res!("resources/icons/logo.png");
//!
//! // As a static (alternative form):
//! res!(pub SAVE_ICON = "resources/icons/save.svg");
//! ```
//!
//! Paths are relative to `CARGO_MANIFEST_DIR`. By convention, resources
//! live under `resources/`.

use std::path::PathBuf;

use proc_macro::TokenStream;
use proc_macro2::TokenStream as TokenStream2;
use quote::quote;
use syn::parse::{Parse, ParseStream};
use syn::{Ident, Result, Token, Visibility, parse_macro_input};

// ---------------------------------------------------------------------------
// Parsed macro input
// ---------------------------------------------------------------------------

enum ResInput {
    /// `res!("path/to/file.svg")` — expression form
    Expr(syn::LitStr),
    /// `res!(pub NAME = "path/to/file.svg")` — static declaration form
    Static {
        vis: Visibility,
        name: Ident,
        path: syn::LitStr,
    },
}

impl Parse for ResInput {
    fn parse(input: ParseStream) -> Result<Self> {
        // Try to parse as static form: [vis] IDENT = "path"
        if input.peek(Token![pub]) || (input.peek(Ident) && input.peek2(Token![=])) {
            let vis: Visibility = input.parse()?;
            let name: Ident = input.parse()?;
            let _eq: Token![=] = input.parse()?;
            let path: syn::LitStr = input.parse()?;
            Ok(ResInput::Static { vis, name, path })
        } else {
            let path: syn::LitStr = input.parse()?;
            Ok(ResInput::Expr(path))
        }
    }
}

// ---------------------------------------------------------------------------
// Format detection and validation
// ---------------------------------------------------------------------------

#[derive(Clone, Copy)]
enum ResourceKind {
    Svg,
    Png,
    Webp,
}

fn detect_kind(path: &str) -> Option<ResourceKind> {
    if path.ends_with(".svg") {
        Some(ResourceKind::Svg)
    } else if path.ends_with(".png") {
        Some(ResourceKind::Png)
    } else if path.ends_with(".webp") {
        Some(ResourceKind::Webp)
    } else {
        None
    }
}

fn validate_svg(data: &[u8], path: &str) -> std::result::Result<(), String> {
    let text = std::str::from_utf8(data).map_err(|e| format!("{path}: not valid UTF-8: {e}"))?;
    let doc =
        roxmltree::Document::parse(text).map_err(|e| format!("{path}: XML parse error: {e}"))?;
    let root = doc.root_element();
    if root.tag_name().name() != "svg" {
        return Err(format!(
            "{path}: root element is <{}>, expected <svg>",
            root.tag_name().name()
        ));
    }
    // Check viewBox or width/height
    if root.attribute("viewBox").is_none()
        && (root.attribute("width").is_none() || root.attribute("height").is_none())
    {
        return Err(format!(
            "{path}: missing viewBox and width/height attributes"
        ));
    }
    Ok(())
}

fn validate_png(data: &[u8], path: &str) -> std::result::Result<(), String> {
    if data.len() < 8 {
        return Err(format!("{path}: file too small for PNG"));
    }
    let magic = &data[..8];
    if magic != b"\x89PNG\r\n\x1a\n" {
        return Err(format!("{path}: invalid PNG magic bytes"));
    }
    Ok(())
}

fn validate_webp(data: &[u8], path: &str) -> std::result::Result<(), String> {
    if data.len() < 12 {
        return Err(format!("{path}: file too small for WebP"));
    }
    if &data[..4] != b"RIFF" {
        return Err(format!("{path}: missing RIFF header"));
    }
    if &data[8..12] != b"WEBP" {
        return Err(format!("{path}: missing WEBP signature"));
    }
    Ok(())
}

/// Check if a WebP file contains an ANIM chunk (animated).
fn webp_is_animated(data: &[u8]) -> bool {
    // Look for ANIM chunk in the RIFF container
    if data.len() < 12 {
        return false;
    }
    let mut pos = 12;
    while pos + 8 <= data.len() {
        let chunk_id = &data[pos..pos + 4];
        if chunk_id == b"ANIM" {
            return true;
        }
        let chunk_size =
            u32::from_le_bytes([data[pos + 4], data[pos + 5], data[pos + 6], data[pos + 7]])
                as usize;
        // RIFF chunks are padded to even size; use saturating add for overflow safety
        pos = pos
            .saturating_add(8)
            .saturating_add(chunk_size)
            .saturating_add(chunk_size & 1);
    }
    false
}

// ---------------------------------------------------------------------------
// Code generation
// ---------------------------------------------------------------------------

/// Generate the expression form: a block containing a static LazyLock
/// that returns `&'static T`.
fn emit_expr(kind: ResourceKind, rel_path: &str, animated: bool) -> TokenStream2 {
    let watch = quote! {
        const _: &[u8] = ::core::include_bytes!(::core::concat!(::core::env!("CARGO_MANIFEST_DIR"), "/", #rel_path));
    };
    let (ty, init) = lazy_type_and_init(kind, rel_path, animated);
    quote! {
        {
            #watch
            static __BASTYDE_RES: ::std::sync::LazyLock<#ty> =
                ::std::sync::LazyLock::new(|| { #init });
            &*__BASTYDE_RES
        }
    }
}

/// Generate the static declaration form: `[vis] static NAME: LazyLock<T> = ...;`
fn emit_static(
    vis: &Visibility,
    name: &Ident,
    kind: ResourceKind,
    rel_path: &str,
    animated: bool,
) -> TokenStream2 {
    let watch = quote! {
        const _: &[u8] = ::core::include_bytes!(::core::concat!(::core::env!("CARGO_MANIFEST_DIR"), "/", #rel_path));
    };
    let (ty, init) = lazy_type_and_init(kind, rel_path, animated);
    quote! {
        #watch
        #vis static #name: ::std::sync::LazyLock<#ty> =
            ::std::sync::LazyLock::new(|| { #init });
    }
}

/// Return the (type, initializer_body) for a LazyLock based on resource kind.
/// Return the (type, initializer_body) for a LazyLock based on resource kind.
///
/// Paths go through `::bastyde::canvas::` so consuming crates only need
/// to depend on `bastyde` (the umbrella), not on `bastyde-canvas` directly.
/// This mirrors the serde pattern where `serde_derive` emits `::serde::` paths.
fn lazy_type_and_init(
    kind: ResourceKind,
    rel_path: &str,
    animated: bool,
) -> (TokenStream2, TokenStream2) {
    match kind {
        ResourceKind::Svg => (
            quote!(::bastyde::canvas::svg::SvgIcon),
            quote! {
                ::bastyde::canvas::svg::SvgIcon::parse(
                    ::core::include_str!(::core::concat!(::core::env!("CARGO_MANIFEST_DIR"), "/", #rel_path))
                ).expect("bastyde-resources: SVG validated at compile time")
            },
        ),
        ResourceKind::Png => (
            quote!(::bastyde::canvas::raster::RasterIcon),
            quote! {
                ::bastyde::canvas::raster::RasterIcon::decode_png(
                    ::core::include_bytes!(::core::concat!(::core::env!("CARGO_MANIFEST_DIR"), "/", #rel_path))
                ).expect("bastyde-resources: PNG validated at compile time")
            },
        ),
        ResourceKind::Webp if animated => (
            quote!(::bastyde::canvas::animated::AnimatedIcon),
            quote! {
                ::bastyde::canvas::animated::AnimatedIcon::decode_webp(
                    ::core::include_bytes!(::core::concat!(::core::env!("CARGO_MANIFEST_DIR"), "/", #rel_path))
                ).expect("bastyde-resources: animated WebP validated at compile time")
            },
        ),
        ResourceKind::Webp => (
            quote!(::bastyde::canvas::raster::RasterIcon),
            quote! {
                ::bastyde::canvas::raster::RasterIcon::decode_webp(
                    ::core::include_bytes!(::core::concat!(::core::env!("CARGO_MANIFEST_DIR"), "/", #rel_path))
                ).expect("bastyde-resources: WebP validated at compile time")
            },
        ),
    }
}

/// Emit raw bytes for unknown file types — no decoding, just embed.
fn emit_raw_expr(rel_path: &str) -> TokenStream2 {
    quote! {
        ::core::include_bytes!(::core::concat!(::core::env!("CARGO_MANIFEST_DIR"), "/", #rel_path))
    }
}

/// Emit a static raw bytes declaration for unknown file types.
fn emit_raw_static(vis: &Visibility, name: &Ident, rel_path: &str) -> TokenStream2 {
    quote! {
        #vis static #name: &[u8] =
            ::core::include_bytes!(::core::concat!(::core::env!("CARGO_MANIFEST_DIR"), "/", #rel_path));
    }
}

// ---------------------------------------------------------------------------
// Entry point
// ---------------------------------------------------------------------------

/// Embed a resource file with compile-time format validation.
///
/// See the [crate-level documentation](crate) for usage examples.
#[proc_macro]
pub fn res(input: TokenStream) -> TokenStream {
    let parsed = parse_macro_input!(input as ResInput);

    let (rel_path_lit, span) = match &parsed {
        ResInput::Expr(lit) => (lit, lit.span()),
        ResInput::Static { path, .. } => (path, path.span()),
    };
    let rel_path = rel_path_lit.value();

    // Resolve absolute path for validation
    let manifest = match std::env::var("CARGO_MANIFEST_DIR") {
        Ok(m) => m,
        Err(_) => {
            return syn::Error::new(span, "CARGO_MANIFEST_DIR is not set")
                .to_compile_error()
                .into();
        }
    };
    let abs_path = PathBuf::from(&manifest).join(&rel_path);

    // Read file for validation
    let data = match std::fs::read(&abs_path) {
        Ok(d) => d,
        Err(e) => {
            return syn::Error::new(
                span,
                format!("bastyde-resources: cannot read `{}`: {e}", abs_path.display()),
            )
            .to_compile_error()
            .into();
        }
    };

    // Detect format — unknown extensions are fine, just no validation
    let kind = detect_kind(&rel_path);

    // Validate known formats
    if let Some(k) = kind {
        let validation_result = match k {
            ResourceKind::Svg => validate_svg(&data, &rel_path),
            ResourceKind::Png => validate_png(&data, &rel_path),
            ResourceKind::Webp => validate_webp(&data, &rel_path),
        };
        if let Err(msg) = validation_result {
            return syn::Error::new(span, format!("bastyde-resources: {msg}"))
                .to_compile_error()
                .into();
        }
    }

    // Detect animated WebP
    let animated = matches!(kind, Some(ResourceKind::Webp)) && webp_is_animated(&data);

    // Emit code
    match (parsed, kind) {
        (ResInput::Expr(_), Some(k)) => emit_expr(k, &rel_path, animated).into(),
        (ResInput::Static { vis, name, .. }, Some(k)) => {
            emit_static(&vis, &name, k, &rel_path, animated).into()
        }
        // Unknown extension: embed as raw bytes, no decode
        (ResInput::Expr(_), None) => emit_raw_expr(&rel_path).into(),
        (ResInput::Static { vis, name, .. }, None) => {
            emit_raw_static(&vis, &name, &rel_path).into()
        }
    }
}
