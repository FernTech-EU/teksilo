// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

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
    use quick_xml::Reader;
    use quick_xml::events::Event;

    let text = std::str::from_utf8(data).map_err(|e| format!("{path}: not valid UTF-8: {e}"))?;
    let mut reader = Reader::from_str(text);
    let mut buf = Vec::new();
    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Start(e)) | Ok(Event::Empty(e)) => {
                let name = e.local_name();
                let name_str = std::str::from_utf8(name.as_ref()).unwrap_or("?");
                if name_str != "svg" {
                    return Err(format!(
                        "{path}: root element is <{name_str}>, expected <svg>"
                    ));
                }
                let mut has_viewbox = false;
                let mut has_width = false;
                let mut has_height = false;
                for attr in e.attributes() {
                    let attr = attr.map_err(|err| format!("{path}: XML parse error: {err}"))?;
                    match attr.key.local_name().as_ref() {
                        b"viewBox" => has_viewbox = true,
                        b"width" => has_width = true,
                        b"height" => has_height = true,
                        _ => {}
                    }
                }
                if !has_viewbox && (!has_width || !has_height) {
                    return Err(format!(
                        "{path}: missing viewBox and width/height attributes"
                    ));
                }
                return Ok(());
            }
            Ok(Event::Eof) => {
                return Err(format!("{path}: empty document, no <svg> root"));
            }
            Ok(_) => {} // skip declaration, comments, doctype, whitespace
            Err(e) => return Err(format!("{path}: XML parse error: {e}")),
        }
        buf.clear();
    }
}

fn validate_png(data: &[u8], path: &str) -> std::result::Result<(), String> {
    const PNG_MAGIC: &[u8; 8] = b"\x89PNG\r\n\x1a\n";
    if data.len() < 8 {
        return Err(format!("{path}: file too small for PNG"));
    }
    if &data[..8] != PNG_MAGIC {
        return Err(format!("{path}: invalid PNG magic bytes"));
    }
    // PNG chunks: 4-byte length (BE), 4-byte type, N-byte data, 4-byte CRC.
    // The first chunk must be IHDR with exactly 13 bytes of data.
    if data.len() < 16 {
        return Err(format!("{path}: truncated PNG (no IHDR chunk header)"));
    }
    if &data[12..16] != b"IHDR" {
        return Err(format!("{path}: first PNG chunk is not IHDR"));
    }
    let ihdr_len = u32::from_be_bytes([data[8], data[9], data[10], data[11]]);
    if ihdr_len != 13 {
        return Err(format!(
            "{path}: PNG IHDR length is {ihdr_len}, expected 13"
        ));
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
                format!(
                    "bastyde-resources: cannot read `{}`: {e}",
                    abs_path.display()
                ),
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

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    // --- validate_png -------------------------------------------------------

    #[test]
    fn png_ok() {
        // Minimal syntactically valid bytes: magic + IHDR header (no data needed beyond the check).
        let mut data = b"\x89PNG\r\n\x1a\n".to_vec(); // magic
        data.extend_from_slice(&13u32.to_be_bytes()); // IHDR length = 13
        data.extend_from_slice(b"IHDR"); // chunk type
        data.extend_from_slice(&[0u8; 17]); // 13 bytes data + 4 bytes CRC
        assert!(validate_png(&data, "ok.png").is_ok());
    }

    #[test]
    fn png_too_small() {
        let err = validate_png(b"\x89PNG", "f.png").unwrap_err();
        assert!(err.contains("file too small"), "{err}");
    }

    #[test]
    fn png_bad_magic() {
        let mut data = [0u8; 16];
        data[0] = 0xFF; // break the magic
        let err = validate_png(&data, "f.png").unwrap_err();
        assert!(err.contains("invalid PNG magic bytes"), "{err}");
    }

    #[test]
    fn png_truncated_before_ihdr() {
        // Valid magic but nothing after it.
        let err = validate_png(b"\x89PNG\r\n\x1a\n", "f.png").unwrap_err();
        assert!(err.contains("truncated PNG"), "{err}");
    }

    #[test]
    fn png_wrong_first_chunk_type() {
        let mut data = b"\x89PNG\r\n\x1a\n".to_vec();
        data.extend_from_slice(&0u32.to_be_bytes()); // length
        data.extend_from_slice(b"IDAT"); // wrong type
        let err = validate_png(&data, "f.png").unwrap_err();
        assert!(err.contains("first PNG chunk is not IHDR"), "{err}");
    }

    #[test]
    fn png_wrong_ihdr_length() {
        let mut data = b"\x89PNG\r\n\x1a\n".to_vec();
        data.extend_from_slice(&5u32.to_be_bytes()); // length = 5, should be 13
        data.extend_from_slice(b"IHDR");
        let err = validate_png(&data, "f.png").unwrap_err();
        assert!(err.contains("IHDR length is 5"), "{err}");
    }

    // --- validate_svg -------------------------------------------------------

    #[test]
    fn svg_ok_viewbox() {
        let xml = br#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24"></svg>"#;
        assert!(validate_svg(xml, "ok.svg").is_ok());
    }

    #[test]
    fn svg_ok_width_height() {
        let xml = br#"<svg xmlns="http://www.w3.org/2000/svg" width="24" height="24"></svg>"#;
        assert!(validate_svg(xml, "ok.svg").is_ok());
    }

    #[test]
    fn svg_not_utf8() {
        let bad = b"\xff\xfe not utf-8";
        let err = validate_svg(bad, "f.svg").unwrap_err();
        assert!(err.contains("not valid UTF-8"), "{err}");
    }

    #[test]
    fn svg_malformed_xml() {
        let err = validate_svg(b"<not closed", "f.svg").unwrap_err();
        assert!(err.contains("XML parse error"), "{err}");
    }

    #[test]
    fn svg_wrong_root() {
        let xml = b"<html><body></body></html>";
        let err = validate_svg(xml, "f.svg").unwrap_err();
        assert!(err.contains("expected <svg>"), "{err}");
    }

    #[test]
    fn svg_missing_viewbox_and_dimensions() {
        let xml = br#"<svg xmlns="http://www.w3.org/2000/svg"></svg>"#;
        let err = validate_svg(xml, "f.svg").unwrap_err();
        assert!(err.contains("missing viewBox"), "{err}");
    }

    // --- validate_webp ------------------------------------------------------

    #[test]
    fn webp_ok() {
        let mut data = b"RIFF".to_vec();
        data.extend_from_slice(&0u32.to_le_bytes()); // file size (unused in check)
        data.extend_from_slice(b"WEBP");
        assert!(validate_webp(&data, "ok.webp").is_ok());
    }

    #[test]
    fn webp_too_small() {
        let err = validate_webp(b"RIFF", "f.webp").unwrap_err();
        assert!(err.contains("file too small"), "{err}");
    }

    #[test]
    fn webp_missing_riff() {
        let data = b"XXXX\x00\x00\x00\x00WEBP";
        let err = validate_webp(data, "f.webp").unwrap_err();
        assert!(err.contains("missing RIFF header"), "{err}");
    }

    #[test]
    fn webp_missing_webp_signature() {
        let data = b"RIFF\x00\x00\x00\x00XXXX";
        let err = validate_webp(data, "f.webp").unwrap_err();
        assert!(err.contains("missing WEBP signature"), "{err}");
    }

    // --- webp_is_animated ---------------------------------------------------

    #[test]
    fn webp_static_has_no_anim_chunk() {
        let mut data = b"RIFF\x00\x00\x00\x00WEBP".to_vec();
        // Add a VP8L chunk (not ANIM)
        data.extend_from_slice(b"VP8L");
        data.extend_from_slice(&4u32.to_le_bytes());
        data.extend_from_slice(&[0u8; 4]);
        assert!(!webp_is_animated(&data));
    }

    #[test]
    fn webp_animated_has_anim_chunk() {
        let mut data = b"RIFF\x00\x00\x00\x00WEBP".to_vec();
        // Prepend a dummy chunk before ANIM to exercise the walker loop
        data.extend_from_slice(b"VP8L");
        data.extend_from_slice(&4u32.to_le_bytes());
        data.extend_from_slice(&[0u8; 4]);
        // Now the real ANIM chunk
        data.extend_from_slice(b"ANIM");
        data.extend_from_slice(&6u32.to_le_bytes());
        data.extend_from_slice(&[0u8; 6]);
        assert!(webp_is_animated(&data));
    }

    #[test]
    fn webp_animated_chunk_walker_does_not_overflow() {
        // Chunk claiming a giant size — must not panic or loop forever.
        let mut data = b"RIFF\x00\x00\x00\x00WEBP".to_vec();
        data.extend_from_slice(b"VP8L");
        data.extend_from_slice(&u32::MAX.to_le_bytes()); // huge chunk size
        // No ANIM follows — the walker must stop cleanly.
        assert!(!webp_is_animated(&data));
    }
}
