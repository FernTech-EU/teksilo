// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Pretty-printer: walk the IR and emit formatted source.
//!
//! Layout rules (v1):
//!
//! - 4-space indent per nesting level.
//! - Elements with a non-empty body emit on multiple lines:
//!   `Type(args) {\n    <items>\n}`. Empty bodies and bodyless elements
//!   stay on one line.
//! - One body item per line. Properties keep their source layout
//!   verbatim — values are sliced from the input so user formatting
//!   inside expressions is preserved.
//! - Property order is preserved. The macro lowering reorders handler
//!   properties to the end of the chain at lower-time; that's a
//!   lowering concern, not the formatter's.
//! - Comments and blank lines between body items are preserved via the
//!   trivia table in source order. Blank lines collapse to single
//!   blanks.
//! - Verbatim source slices (Rust expressions, structural forms) are
//!   reindented to the current depth: the first line lands at the
//!   current column; later lines are dedented to a common minimum then
//!   shifted to the body's indent + one continuation level.

use std::ops::Range;

use proc_macro2::TokenStream;
use quote::ToTokens;
use teksilo_parse::{
    BodyItem, PropArg, TeksiElement, TeksiElse, TeksiFor, TeksiIf, TeksiMatch, TeksiRoot,
};

use crate::spans::ts_byte_range;
use crate::trivia::{Trivia, TriviaKind};

const INDENT: &str = "    ";

pub fn print(source: &str, root: &TeksiRoot, trivia: &[Trivia]) -> String {
    let mut p = Printer {
        source,
        trivia,
        trivia_idx: 0,
        out: String::with_capacity(source.len() + source.len() / 8),
        indent: 0,
        cursor: 0,
    };
    p.print_root(root);
    // Drain any trailing trivia (a final comment after the last body item).
    p.drain_trivia_until(source.len());
    // Trim a trailing newline the printer may have emitted past the last
    // closing brace; format_block returns the body without a final \n.
    while p.out.ends_with('\n') {
        p.out.pop();
    }
    p.out
}

struct Printer<'a> {
    source: &'a str,
    trivia: &'a [Trivia],
    trivia_idx: usize,
    out: String,
    indent: usize,
    cursor: usize,
}

impl<'a> Printer<'a> {
    fn write(&mut self, s: &str) {
        self.out.push_str(s);
    }

    fn write_indent(&mut self) {
        for _ in 0..self.indent {
            self.out.push_str(INDENT);
        }
    }

    fn newline(&mut self) {
        self.out.push('\n');
    }

    /// Drain trivia entries with `offset < target`, emitting each on its
    /// own indented line. Skips trivia already consumed by a verbatim
    /// slice (those with offset < cursor). Advances cursor to `target`.
    fn drain_trivia_until(&mut self, target: usize) {
        while self.trivia_idx < self.trivia.len() {
            let t = &self.trivia[self.trivia_idx];
            if t.offset >= target {
                break;
            }
            if t.offset >= self.cursor {
                self.emit_trivia(t);
            }
            self.trivia_idx += 1;
        }
        self.cursor = self.cursor.max(target);
    }

    fn emit_trivia(&mut self, t: &Trivia) {
        // Trivia always lands on its own line at current indent. If the
        // last char already ends a line we don't add one.
        let needs_nl = !self.out.ends_with('\n') && !self.out.is_empty();
        match &t.kind {
            TriviaKind::LineComment(text) => {
                if needs_nl {
                    self.newline();
                }
                self.write_indent();
                self.write("//");
                if !text.is_empty() && !text.starts_with(char::is_whitespace) {
                    self.write(" ");
                }
                self.write(text.trim_end());
                self.newline();
            }
            TriviaKind::BlockComment(text) => {
                if needs_nl {
                    self.newline();
                }
                self.write_indent();
                self.write("/*");
                self.write(text);
                self.write("*/");
                self.newline();
            }
            TriviaKind::BlankLine => {
                // Avoid duplicate blanks.
                if !self.out.ends_with("\n\n") && !self.out.is_empty() {
                    if !self.out.ends_with('\n') {
                        self.newline();
                    }
                    self.newline();
                }
            }
        }
    }

    fn print_root(&mut self, root: &TeksiRoot) {
        if let Some(ctx) = &root.ctx {
            self.write(&ctx.to_string());
            self.write(" => ");
        }
        self.print_element(&root.root);
    }

    fn print_element(&mut self, e: &TeksiElement) {
        // Drain trivia preceding the element header.
        let head_start = e.head_span.byte_range().start;
        self.drain_trivia_until(head_start);

        // Header: type path + args.
        self.write(&path_text(self.source, &e.type_path));
        if let Some(close) = e.args_close {
            self.write("(");
            for (i, arg) in e.args.iter().enumerate() {
                if i > 0 {
                    self.write(", ");
                }
                // Use the dedent+reindent path so multi-line arg
                // expressions (e.g. a string literal split across
                // lines or a nested call broken at parens) anchor
                // their continuations to the current printer indent
                // rather than carrying source-absolute spacing. The
                // latter would compound with `reindent_block` on
                // every successive run.
                self.write_verbatim_multiline(&arg.to_token_stream());
            }
            self.write(")");
            // Advance cursor exactly past the closing `)`.
            self.cursor = self.cursor.max(close.byte_range().end);
        }

        // Treat element as bodyless (no `{}`) if the user didn't write
        // braces. An empty `{}` in source still has body_close = Some(...)
        // and the body items vec is empty — we collapse to bodyless form
        // unconditionally because the two are semantically identical.
        if e.body.is_empty() {
            // If there were body braces but no items, advance cursor past
            // the closing `}` so trailing trivia inside the empty body
            // doesn't get re-emitted later.
            if let Some(close) = e.body_close {
                self.cursor = self.cursor.max(close.byte_range().end);
            }
            return;
        }

        self.write(" {");
        self.newline();
        self.indent += 1;

        for (idx, item) in e.body.iter().enumerate() {
            let item_start = item_byte_range(item).start;
            self.drain_trivia_until(item_start);
            // Avoid leading blank line at the start of a body.
            if idx == 0 {
                self.trim_leading_blank();
            }
            self.write_indent();
            self.print_body_item(item);
            if !self.out.ends_with('\n') {
                self.newline();
            }
        }

        // Drain any trivia between the last body item and the closing
        // `}`, anchored on the exact close-brace span from the IR.
        if let Some(close) = e.body_close {
            self.drain_trivia_until(close.byte_range().start);
            self.trim_trailing_blank();
            self.cursor = self.cursor.max(close.byte_range().end);
        }

        self.indent -= 1;
        if !self.out.ends_with('\n') {
            self.newline();
        }
        self.write_indent();
        self.write("}");
    }

    fn trim_leading_blank(&mut self) {
        while self.out.ends_with("\n\n") {
            self.out.pop();
        }
    }

    fn trim_trailing_blank(&mut self) {
        while self.out.ends_with("\n\n") {
            self.out.pop();
        }
    }

    fn print_body_item(&mut self, item: &BodyItem) {
        match item {
            BodyItem::Property(p) => self.print_property(p),
            BodyItem::Child(el) => self.print_element(el),
            BodyItem::Binding { name, element } => {
                self.write(&name.to_string());
                self.write(" = ");
                self.print_element(element);
            }
            BodyItem::Escape { expr, .. } => {
                self.write("#{ ");
                self.write(&verbatim_slice(self.source, &expr.to_token_stream()));
                self.write(" }");
                let r = item_byte_range(item);
                self.cursor = self.cursor.max(r.end);
            }
            BodyItem::Let(local) => {
                self.write_verbatim_multiline(&local.to_token_stream());
                let r = item_byte_range(item);
                self.cursor = self.cursor.max(r.end);
            }
            BodyItem::Rust { block, .. } => {
                self.write("rust ");
                self.write_verbatim_multiline(&block.to_token_stream());
                let r = item_byte_range(item);
                self.cursor = self.cursor.max(r.end);
            }
            BodyItem::If(if_) => {
                self.write_verbatim_multiline(&teksilo_if_token_stream(if_));
                let r = item_byte_range(item);
                self.cursor = self.cursor.max(r.end);
            }
            BodyItem::Match(m) => {
                self.write_verbatim_multiline(&teksilo_match_token_stream(m));
                let r = item_byte_range(item);
                self.cursor = self.cursor.max(r.end);
            }
            BodyItem::For(f) => {
                self.write_verbatim_multiline(&teksilo_for_token_stream(f));
                let r = item_byte_range(item);
                self.cursor = self.cursor.max(r.end);
            }
            BodyItem::Spread { expr, .. } => {
                self.write("..");
                self.write(&verbatim_slice(self.source, &expr.to_token_stream()));
                let r = item_byte_range(item);
                self.cursor = self.cursor.max(r.end);
            }
        }
    }

    fn print_property(&mut self, p: &teksilo_parse::TeksiProperty) {
        self.write(&p.name.to_string());
        if p.args.is_empty() {
            return;
        }
        self.write(": ");
        for (i, arg) in p.args.iter().enumerate() {
            if i > 0 {
                self.write(", ");
            }
            self.print_prop_arg(arg);
        }
    }

    fn print_prop_arg(&mut self, arg: &PropArg) {
        match arg {
            PropArg::Expr(e) => {
                self.write_verbatim_multiline(&e.to_token_stream());
                if let Some(end) = ts_end(&e.to_token_stream()) {
                    self.cursor = self.cursor.max(end);
                }
            }
            PropArg::Element(el) => self.print_element(el),
            PropArg::Escape(e) => {
                self.write("#{ ");
                self.write(&verbatim_slice(self.source, &e.to_token_stream()));
                self.write(" }");
                if let Some(end) = ts_end(&e.to_token_stream()) {
                    self.cursor = self.cursor.max(end + 1);
                }
            }
            PropArg::Binding { name, element } => {
                self.write(&name.to_string());
                self.write(" = ");
                self.print_element(element);
            }
        }
    }

    /// Emit a verbatim source slice with multi-line dedent + reindent
    /// to the current body depth.
    ///
    /// The slice's min-indent line (typically the closing `}` of a
    /// structural form or closure) anchors at `self.indent` — the same
    /// column the form's keyword sits at on the line `write_indent`
    /// already laid down. Deeper lines (arms / bodies) end up at
    /// `self.indent + their relative depth`, preserving the source's
    /// internal indentation hierarchy. Round-trip stable: a re-format
    /// produces a slice with the same leading whitespace pattern.
    fn write_verbatim_multiline(&mut self, ts: &TokenStream) {
        let Some(range) = ts_byte_range(ts) else {
            return;
        };
        let slice = &self.source[range.clone()];
        if !slice.contains('\n') {
            self.write(slice);
            return;
        }
        let cont_indent = INDENT.repeat(self.indent);
        let lines: Vec<&str> = slice.split('\n').collect();
        // First line emitted as-is (we're already at the current column).
        self.write(lines[0]);
        // Find the minimum leading whitespace across non-empty trailing lines.
        let min_indent = lines[1..]
            .iter()
            .filter(|l| !l.trim().is_empty())
            .map(|l| l.len() - l.trim_start().len())
            .min()
            .unwrap_or(0);
        // Dedent each non-first line by min_indent and prepend cont_indent.
        // We don't special-case lines starting with `}` — uniform dedent
        // + reindent is mechanical and round-trip stable.
        for line in &lines[1..] {
            self.newline();
            if line.trim().is_empty() {
                continue;
            }
            let stripped = if line.len() >= min_indent {
                &line[min_indent..]
            } else {
                line.trim_start()
            };
            self.write(&cont_indent);
            self.write(stripped);
        }
    }
}

// ---------------------------------------------------------------------------
// Span / source-slice helpers
// ---------------------------------------------------------------------------

fn ts_end(ts: &TokenStream) -> Option<usize> {
    ts_byte_range(ts).map(|r| r.end)
}

fn verbatim_slice(source: &str, ts: &TokenStream) -> String {
    match ts_byte_range(ts) {
        Some(r) => source[r].to_string(),
        None => String::new(),
    }
}

fn path_text(source: &str, path: &syn::Path) -> String {
    verbatim_slice(source, &path.to_token_stream())
}

// Synthesize a token stream covering each structural form's full span.
// We don't need to PRINT the tokens — only to compute byte extents — so
// the emitted token order doesn't matter for correctness.

fn teksilo_if_token_stream(if_: &TeksiIf) -> TokenStream {
    let mut ts = TokenStream::new();
    if_.cond.to_tokens(&mut ts);
    element_to_tokens(&if_.then, &mut ts);
    push_anchor(&mut ts, if_.body_close, "__bc");
    if let Some(b) = &if_.else_branch {
        match &**b {
            TeksiElse::ElseIf(nested) => teksilo_if_to_tokens(nested, &mut ts),
            TeksiElse::Element {
                element,
                body_close,
            } => {
                element_to_tokens(element, &mut ts);
                push_anchor(&mut ts, *body_close, "__bc");
            }
        }
    }
    // Anchor the `if` keyword span too.
    push_anchor(&mut ts, if_.span, "if");
    ts
}

fn teksilo_if_to_tokens(if_: &TeksiIf, ts: &mut TokenStream) {
    if_.cond.to_tokens(ts);
    element_to_tokens(&if_.then, ts);
    push_anchor(ts, if_.body_close, "__bc");
    if let Some(b) = &if_.else_branch {
        match &**b {
            TeksiElse::ElseIf(nested) => teksilo_if_to_tokens(nested, ts),
            TeksiElse::Element {
                element,
                body_close,
            } => {
                element_to_tokens(element, ts);
                push_anchor(ts, *body_close, "__bc");
            }
        }
    }
}

fn teksilo_match_token_stream(m: &TeksiMatch) -> TokenStream {
    let mut ts = TokenStream::new();
    m.scrutinee.to_tokens(&mut ts);
    for arm in &m.arms {
        arm.pat.to_tokens(&mut ts);
        if let Some((_, g)) = &arm.guard {
            g.to_tokens(&mut ts);
        }
        element_to_tokens(&arm.element, &mut ts);
    }
    push_anchor(&mut ts, m.span, "match");
    push_anchor(&mut ts, m.body_close, "__bc");
    ts
}

fn teksilo_for_token_stream(f: &TeksiFor) -> TokenStream {
    let mut ts = TokenStream::new();
    f.pat.to_tokens(&mut ts);
    f.iter.to_tokens(&mut ts);
    for l in &f.lets {
        l.to_tokens(&mut ts);
    }
    element_to_tokens(&f.element, &mut ts);
    push_anchor(&mut ts, f.span, "for");
    push_anchor(&mut ts, f.body_close, "__bc");
    ts
}

/// Anchor a span into a TokenStream so `ts_byte_range` accounts for it.
/// The emitted ident is never printed — it only contributes its byte
/// range to the union computed by `walk_extents`.
fn push_anchor(ts: &mut TokenStream, span: proc_macro2::Span, name: &str) {
    use proc_macro2::{Ident, TokenTree};
    ts.extend(std::iter::once(TokenTree::Ident(Ident::new(name, span))));
}

fn element_to_tokens(e: &TeksiElement, ts: &mut TokenStream) {
    e.type_path.to_tokens(ts);
    for a in &e.args {
        a.to_tokens(ts);
    }
    for item in &e.body {
        body_item_to_tokens(item, ts);
    }
}

fn body_item_to_tokens(item: &BodyItem, ts: &mut TokenStream) {
    match item {
        BodyItem::Property(p) => {
            p.name.to_tokens(ts);
            for arg in &p.args {
                prop_arg_to_tokens(arg, ts);
            }
        }
        BodyItem::Child(el) => element_to_tokens(el, ts),
        BodyItem::Binding { name, element } => {
            name.to_tokens(ts);
            element_to_tokens(element, ts);
        }
        BodyItem::Escape { expr, span } => {
            use proc_macro2::{Ident, TokenTree};
            ts.extend(std::iter::once(TokenTree::Ident(Ident::new(
                "__escape", *span,
            ))));
            expr.to_tokens(ts);
        }
        BodyItem::Let(l) => l.to_tokens(ts),
        BodyItem::Rust { block, span, .. } => {
            use proc_macro2::{Ident, TokenTree};
            ts.extend(std::iter::once(TokenTree::Ident(Ident::new("rust", *span))));
            block.to_tokens(ts);
        }
        BodyItem::If(if_) => ts.extend(teksilo_if_token_stream(if_)),
        BodyItem::Match(m) => ts.extend(teksilo_match_token_stream(m)),
        BodyItem::For(f) => ts.extend(teksilo_for_token_stream(f)),
        BodyItem::Spread { expr, span } => {
            use proc_macro2::{Ident, TokenTree};
            ts.extend(std::iter::once(TokenTree::Ident(Ident::new(
                "__spread", *span,
            ))));
            expr.to_tokens(ts);
        }
    }
}

fn prop_arg_to_tokens(arg: &PropArg, ts: &mut TokenStream) {
    match arg {
        PropArg::Expr(e) => e.to_tokens(ts),
        PropArg::Element(el) => element_to_tokens(el, ts),
        PropArg::Escape(e) => e.to_tokens(ts),
        PropArg::Binding { name, element } => {
            name.to_tokens(ts);
            element_to_tokens(element, ts);
        }
    }
}

fn item_byte_range(item: &BodyItem) -> Range<usize> {
    let mut ts = TokenStream::new();
    body_item_to_tokens(item, &mut ts);
    ts_byte_range(&ts).unwrap_or(0..0)
}
