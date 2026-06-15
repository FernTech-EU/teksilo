// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Formatter for the `bati!` DSL.
//!
//! Pure library — the CLI lives in `cargo-bastyde-fmt`. Two entry points:
//!
//! - [`format_block`] — format the body of a single `bati!(...)` macro
//!   invocation. Input is the contents inside the parens, including any
//!   `ctx => ` preamble.
//! - [`format_file`] — format every `bati!(...)` invocation in a Rust
//!   source file. Returns the full file content with each macro body
//!   replaced by its formatted form. Source outside `bati!` blocks is
//!   untouched.
//!
//! Comments and blank lines between body items are preserved via a
//! byte-range trivia pass over the original source. Rust expressions
//! embedded in the DSL (positional args, property values, escape
//! exprs, etc.) are sliced verbatim from source — preserving any
//! user formatting inside them.

mod printer;
mod spans;
mod trivia;
mod visit;

use proc_macro2::TokenStream;
use std::str::FromStr;

// Re-exports for downstream tooling (e.g. `bastyde-designer`) that needs
// to locate, scan, and re-indent `bati!` blocks rather than just format
// whole files.
pub use trivia::{Trivia, TriviaKind, scan as scan_trivia};
pub use visit::{BatiMacroEdit, find_bastyde_macros};

/// Formatter configuration. Empty in v1; reserved for future style knobs.
#[derive(Debug, Default, Clone)]
pub struct FmtConfig {}

/// Errors produced by the formatter.
#[derive(Debug, thiserror::Error)]
pub enum FmtError {
    /// The `bati!` body failed to parse. Carries the underlying syn error
    /// with its span pointing into the input source.
    #[error("bati! body parse error: {0}")]
    Parse(syn::Error),
    /// The host Rust file failed to parse. Only produced by [`format_file`].
    #[error("host file parse error: {0}")]
    HostParse(syn::Error),
}

/// Format the body of a single `bati!(...)` invocation.
///
/// `source` is the text between the macro parens — for `bati!(ctx => VStack {})`
/// pass `"ctx => VStack {}"`. The returned string is the reformatted body
/// without surrounding parens or `bati!`.
///
/// Output uses LF newlines unconditionally. CRLF input is parsed
/// correctly (the trivia scanner handles `\r\n`) but emitted as LF.
/// Callers that need to preserve CRLF should use [`format_file`],
/// which detects the host file's line ending convention and applies
/// it to the formatter's output.
pub fn format_block(source: &str, _config: &FmtConfig) -> Result<String, FmtError> {
    let tokens = TokenStream::from_str(source).map_err(|e| {
        FmtError::Parse(syn::Error::new(
            proc_macro2::Span::call_site(),
            e.to_string(),
        ))
    })?;
    let root = bastyde_parse::parse_root(tokens.clone()).map_err(FmtError::Parse)?;
    let trivia = trivia::scan(source, &tokens);
    Ok(printer::print(source, &root, &trivia))
}

/// Format every `bati!(...)` invocation in a Rust source file.
///
/// `source` is the full Rust source text. The returned string is the
/// same source with each `bati!` macro body replaced by its formatted
/// form. Source outside `bati!` blocks is byte-for-byte unchanged. The
/// host file's line ending convention (LF or CRLF) is detected from
/// the first newline in source and applied to every newline the
/// formatter emits, so a CRLF file round-trips as CRLF.
pub fn format_file(source: &str, config: &FmtConfig) -> Result<String, FmtError> {
    let file = syn::parse_file(source).map_err(FmtError::HostParse)?;
    let macros = visit::find_bastyde_macros(source, &file);

    if macros.is_empty() {
        return Ok(source.to_string());
    }

    let line_ending = detect_line_ending(source);

    // Apply edits from end to start so earlier byte offsets remain valid.
    let mut edits: Vec<visit::BatiMacroEdit> = macros;
    edits.sort_by_key(|e| std::cmp::Reverse(e.body_range.start));

    let mut out = source.to_string();
    for edit in edits {
        let body_src = &source[edit.body_range.clone()];
        let formatted = match format_block(body_src, config) {
            Ok(f) => f,
            // Block is syntactically invalid (e.g. intentional fail-test).
            // Leave it unchanged rather than aborting the entire file.
            Err(FmtError::Parse(_)) => continue,
            Err(e) => return Err(e),
        };
        let reindented = reindent_block(&formatted, edit.base_indent);
        let with_endings = match line_ending {
            LineEnding::Lf => reindented,
            LineEnding::Crlf => normalize_to_crlf(&reindented),
        };
        out.replace_range(edit.body_range, &with_endings);
    }
    Ok(out)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LineEnding {
    Lf,
    Crlf,
}

/// Inspect the first `\n` in `source`. If it's preceded by `\r`, the
/// file uses CRLF; otherwise LF. Files with no newline default to LF.
fn detect_line_ending(source: &str) -> LineEnding {
    match source.find('\n') {
        Some(idx) if idx > 0 && source.as_bytes()[idx - 1] == b'\r' => LineEnding::Crlf,
        _ => LineEnding::Lf,
    }
}

/// Replace bare `\n` (not already preceded by `\r`) with `\r\n`.
/// Idempotent — already-CRLF input passes through unchanged.
fn normalize_to_crlf(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + s.matches('\n').count());
    let mut prev = '\0';
    for c in s.chars() {
        if c == '\n' && prev != '\r' {
            out.push('\r');
        }
        out.push(c);
        prev = c;
    }
    out
}

/// Re-indent a formatted block so its outermost `}` lands at the same
/// column the user already had in source (see
/// `visit::observed_body_indent`). The formatter emits with column-0
/// indents internally; this shifts every line except the first by
/// `base_indent` spaces. Newlines emitted by the formatter remain
/// `\n`-only — line-ending normalization (LF↔CRLF) is the caller's
/// responsibility.
pub fn reindent_block(body: &str, base_indent: usize) -> String {
    if base_indent == 0 || !body.contains('\n') {
        return body.to_string();
    }
    let pad = " ".repeat(base_indent);
    let mut out = String::with_capacity(body.len() + base_indent * body.matches('\n').count());
    for (i, line) in body.split('\n').enumerate() {
        if i > 0 {
            out.push('\n');
            if !line.is_empty() {
                out.push_str(&pad);
            }
        }
        out.push_str(line);
    }
    out
}
