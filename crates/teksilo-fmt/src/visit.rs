// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Find `teksu!(...)` macro invocations in a Rust source file.
//!
//! Returned edits carry the byte range of the macro body (the contents
//! inside the macro's delimiter group, exclusive of the open/close
//! delimiters) plus the column the macro call started at — used to
//! re-indent multi-line formatted output back to source position.

use std::ops::Range;
use syn::visit::Visit;

use crate::spans;

#[derive(Debug, Clone)]
pub struct TeksiMacroEdit {
    /// Byte range of the macro body (exclusive of the open/close delimiters).
    pub body_range: Range<usize>,
    /// Column the macro call started at. Continuation lines of the
    /// formatted body are shifted by this many spaces.
    pub base_indent: usize,
}

pub fn find_teksilo_macros(source: &str, file: &syn::File) -> Vec<TeksiMacroEdit> {
    let mut visitor = TeksiMacroVisitor {
        source,
        edits: Vec::new(),
    };
    visitor.visit_file(file);
    visitor.edits
}

struct TeksiMacroVisitor<'a> {
    source: &'a str,
    edits: Vec<TeksiMacroEdit>,
}

impl<'a, 'ast> Visit<'ast> for TeksiMacroVisitor<'a> {
    fn visit_macro(&mut self, m: &'ast syn::Macro) {
        let last = m.path.segments.last();
        let is_teksi = last.map(|s| s.ident == "teksu").unwrap_or(false);
        if !is_teksi {
            syn::visit::visit_macro(self, m);
            return;
        }
        if let Some(range) = spans::ts_byte_range(&m.tokens) {
            let base_indent = observed_body_indent(self.source, &range);
            self.edits.push(TeksiMacroEdit {
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
/// - If the body spans several lines, return the leading-whitespace count
///   of the LAST non-empty line. That's the line carrying the outermost
///   `}` in the user's source, so the formatted output's matching `}` will
///   land at the same column.
/// - If the body is on one line, there is no such brace line to measure.
///   Fall back to the indentation of the line the macro call sits on, so a
///   body that the printer expands still lands under its `teksu!(` rather
///   than at column 0.
fn observed_body_indent(source: &str, body_range: &Range<usize>) -> usize {
    let body = &source[body_range.clone()];
    if body.contains('\n') {
        return body
            .lines()
            .rev()
            .find(|l| !l.trim().is_empty())
            .map(|l| l.len() - l.trim_start().len())
            .unwrap_or(0);
    }
    // Inline body: measure the line the `teksu!` call itself starts on.
    let line_start = source[..body_range.start]
        .rfind('\n')
        .map_or(0, |nl| nl + 1);
    let line = &source[line_start..body_range.start];
    line.len() - line.trim_start().len()
}
