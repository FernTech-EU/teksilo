//! Formatter for the `fern!` DSL.
//!
//! Pure library — the CLI lives in `cargo-fern-fmt`. Two entry points:
//!
//! - [`format_block`] — format the body of a single `fern!(...)` macro
//!   invocation. Input is the contents inside the parens, including any
//!   `ctx => ` preamble.
//! - [`format_file`] — format every `fern!(...)` invocation in a Rust
//!   source file. Returns the full file content with each macro body
//!   replaced by its formatted form. Source outside `fern!` blocks is
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

/// Formatter configuration. Empty in v1; reserved for future style knobs.
#[derive(Debug, Default, Clone)]
pub struct FmtConfig {}

/// Errors produced by the formatter.
#[derive(Debug)]
pub enum FmtError {
    /// The `fern!` body failed to parse. Carries the underlying syn error
    /// with its span pointing into the input source.
    Parse(syn::Error),
    /// The host Rust file failed to parse. Only produced by [`format_file`].
    HostParse(syn::Error),
}

impl std::fmt::Display for FmtError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            FmtError::Parse(e) => write!(f, "fern! body parse error: {e}"),
            FmtError::HostParse(e) => write!(f, "host file parse error: {e}"),
        }
    }
}

impl std::error::Error for FmtError {}

/// Format the body of a single `fern!(...)` invocation.
///
/// `source` is the text between the macro parens — for `fern!(ctx => VStack {})`
/// pass `"ctx => VStack {}"`. The returned string is the reformatted body
/// without surrounding parens or `fern!`.
pub fn format_block(source: &str, _config: &FmtConfig) -> Result<String, FmtError> {
    let tokens = TokenStream::from_str(source).map_err(|e| {
        FmtError::Parse(syn::Error::new(proc_macro2::Span::call_site(), e.to_string()))
    })?;
    let root = fern_parse::parse_root(tokens.clone()).map_err(FmtError::Parse)?;
    let trivia = trivia::scan(source, &tokens);
    Ok(printer::print(source, &root, &trivia))
}

/// Format every `fern!(...)` invocation in a Rust source file.
///
/// `source` is the full Rust source text. The returned string is the
/// same source with each `fern!` macro body replaced by its formatted
/// form. Source outside `fern!` blocks is byte-for-byte unchanged.
pub fn format_file(source: &str, config: &FmtConfig) -> Result<String, FmtError> {
    let file = syn::parse_file(source).map_err(FmtError::HostParse)?;
    let macros = visit::find_fern_macros(source, &file);

    if macros.is_empty() {
        return Ok(source.to_string());
    }

    // Apply edits from end to start so earlier byte offsets remain valid.
    let mut edits: Vec<visit::FernMacroEdit> = macros;
    edits.sort_by_key(|e| std::cmp::Reverse(e.body_range.start));

    let mut out = source.to_string();
    for edit in edits {
        let body_src = &source[edit.body_range.clone()];
        let formatted = format_block(body_src, config)?;
        let reindented = reindent_block(&formatted, edit.base_indent);
        out.replace_range(edit.body_range, &reindented);
    }
    Ok(out)
}

/// Re-indent a formatted block so it lines up with the column where the
/// macro body started in the host file. The formatter emits with column 0
/// indents internally; this shifts every line (except the first) by
/// `base_indent` spaces so the closing `)` of the macro call lands at a
/// sensible column.
fn reindent_block(body: &str, base_indent: usize) -> String {
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
