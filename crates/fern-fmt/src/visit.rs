//! Find `fern!(...)` macro invocations in a Rust source file.
//!
//! Returned edits carry the byte range of the macro body (the contents
//! inside the macro's delimiter group, exclusive of the open/close
//! delimiters) plus the column the macro call started at — used to
//! re-indent multi-line formatted output back to source position.

use std::ops::Range;
use syn::visit::Visit;

use crate::spans;

#[derive(Debug, Clone)]
pub struct FernMacroEdit {
    /// Byte range of the macro body (exclusive of the open/close delimiters).
    pub body_range: Range<usize>,
    /// Column the macro call started at. Continuation lines of the
    /// formatted body are shifted by this many spaces.
    pub base_indent: usize,
}

pub fn find_fern_macros(source: &str, file: &syn::File) -> Vec<FernMacroEdit> {
    let mut visitor = FernMacroVisitor {
        source,
        edits: Vec::new(),
    };
    visitor.visit_file(file);
    visitor.edits
}

struct FernMacroVisitor<'a> {
    source: &'a str,
    edits: Vec<FernMacroEdit>,
}

impl<'a, 'ast> Visit<'ast> for FernMacroVisitor<'a> {
    fn visit_macro(&mut self, m: &'ast syn::Macro) {
        let last = m.path.segments.last();
        let is_fern = last.map(|s| s.ident == "fern").unwrap_or(false);
        if !is_fern {
            syn::visit::visit_macro(self, m);
            return;
        }
        if let Some(range) = spans::ts_byte_range(&m.tokens) {
            let base_indent = observed_body_indent(self.source, &range);
            self.edits.push(FernMacroEdit {
                body_range: range,
                base_indent,
            });
        }
        syn::visit::visit_macro(self, m);
    }
}

/// Pick the column to splice the formatted body back at.
///
/// The formatter emits with column-0 indents internally; we shift each
/// continuation line by this amount so the result aligns with where
/// the user already had the body's outer brace.
///
/// - If the body has no newline, return 0 — the body was inline with
///   `fern!(` and we keep it inline.
/// - Otherwise, return the leading-whitespace count of the LAST
///   non-empty line. That's the line carrying the outermost `}` in
///   the user's source, so the formatted output's matching `}` will
///   land at the same column.
fn observed_body_indent(source: &str, body_range: &Range<usize>) -> usize {
    let body = &source[body_range.clone()];
    if !body.contains('\n') {
        return 0;
    }
    body.lines()
        .rev()
        .find(|l| !l.trim().is_empty())
        .map(|l| l.len() - l.trim_start().len())
        .unwrap_or(0)
}
