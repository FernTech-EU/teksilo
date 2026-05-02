//! Byte-range trivia scanner.
//!
//! `syn::ParseStream` discards comments and whitespace before they reach
//! the IR. To preserve them across reformat we run a separate pass over
//! the original source: we walk the leaf tokens of the input
//! `TokenStream`, record their byte ranges, then scan every gap between
//! consecutive tokens for `//` line comments, `/* */` block comments,
//! and blank lines.
//!
//! The printer drains this table by byte offset: before emitting any
//! IR node, it flushes all trivia whose offset falls below the cursor.
//! After emitting a verbatim source slice (e.g. an embedded Rust
//! expression), the cursor advances to the slice's end and trivia
//! inside that slice is considered already represented.

use proc_macro2::{TokenStream, TokenTree};
use std::ops::Range;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TriviaKind {
    /// `// text` — the stored text excludes the leading `//` and any
    /// trailing newline.
    LineComment(String),
    /// `/* text */` — the stored text excludes the surrounding markers.
    BlockComment(String),
    /// One blank line. Multiple consecutive blank lines collapse to a
    /// single marker.
    BlankLine,
}

#[derive(Debug, Clone)]
pub struct Trivia {
    /// Byte offset of the trivia start in the original source.
    pub offset: usize,
    pub kind: TriviaKind,
}

/// Scan `source` for trivia between tokens of `tokens`. Returned vec is
/// sorted by ascending offset.
pub fn scan(source: &str, tokens: &TokenStream) -> Vec<Trivia> {
    let mut ranges: Vec<Range<usize>> = Vec::new();
    collect_byte_ranges(tokens, &mut ranges);
    ranges.sort_by_key(|r| r.start);

    let mut trivia = Vec::new();
    let mut cursor = 0usize;
    for r in &ranges {
        if cursor < r.start {
            scan_gap(source, cursor..r.start, &mut trivia);
        }
        cursor = cursor.max(r.end);
    }
    if cursor < source.len() {
        scan_gap(source, cursor..source.len(), &mut trivia);
    }
    trivia
}

fn collect_byte_ranges(ts: &TokenStream, out: &mut Vec<Range<usize>>) {
    for tt in ts.clone() {
        match tt {
            TokenTree::Group(g) => {
                // Use span_open / span_close so the inside-of-group bytes
                // remain "gap" territory and inner-group comments are
                // discoverable.
                out.push(g.span_open().byte_range());
                collect_byte_ranges(&g.stream(), out);
                out.push(g.span_close().byte_range());
            }
            TokenTree::Ident(i) => out.push(i.span().byte_range()),
            TokenTree::Punct(p) => out.push(p.span().byte_range()),
            TokenTree::Literal(l) => out.push(l.span().byte_range()),
        }
    }
}

fn scan_gap(source: &str, range: Range<usize>, out: &mut Vec<Trivia>) {
    let base = range.start;
    let bytes = source.as_bytes();
    let end = range.end.min(bytes.len());
    let mut i = base;
    let mut blank_emitted_for_run = false;
    while i < end {
        let b = bytes[i];
        if b == b'/' && i + 1 < end && bytes[i + 1] == b'/' {
            let start = i;
            i += 2;
            while i < end && bytes[i] != b'\n' {
                i += 1;
            }
            let inner = source[start + 2..i].trim_end_matches('\r').to_string();
            out.push(Trivia {
                offset: start,
                kind: TriviaKind::LineComment(inner),
            });
            blank_emitted_for_run = false;
        } else if b == b'/' && i + 1 < end && bytes[i + 1] == b'*' {
            let start = i;
            i += 2;
            while i + 1 < end && !(bytes[i] == b'*' && bytes[i + 1] == b'/') {
                i += 1;
            }
            let inner_end = i;
            if i + 1 < end {
                i += 2;
            } else {
                i = end;
            }
            let inner = source[start + 2..inner_end].to_string();
            out.push(Trivia {
                offset: start,
                kind: TriviaKind::BlockComment(inner),
            });
            blank_emitted_for_run = false;
        } else if b == b'\n' {
            // Look ahead through whitespace for a second newline.
            let mut j = i + 1;
            let mut found = false;
            while j < end {
                match bytes[j] {
                    b' ' | b'\t' | b'\r' => j += 1,
                    b'\n' => {
                        found = true;
                        break;
                    }
                    _ => break,
                }
            }
            if found && !blank_emitted_for_run {
                out.push(Trivia {
                    offset: i,
                    kind: TriviaKind::BlankLine,
                });
                blank_emitted_for_run = true;
            }
            i += 1;
        } else if b.is_ascii_whitespace() {
            i += 1;
        } else {
            // Defensive: a non-whitespace, non-comment byte in a gap means
            // either span coverage was incomplete (proc-macro2 bug) or a
            // multi-byte UTF-8 prefix landed here. Skip one byte and move
            // on; we never emit unrecognized bytes as trivia.
            i += 1;
        }
    }
    let _ = blank_emitted_for_run;
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::str::FromStr;

    fn scan_str(s: &str) -> Vec<Trivia> {
        let ts = TokenStream::from_str(s).unwrap();
        scan(s, &ts)
    }

    #[test]
    fn line_comment_between_tokens() {
        let s = "a // hi\nb";
        let t = scan_str(s);
        assert_eq!(t.len(), 1);
        assert!(matches!(&t[0].kind, TriviaKind::LineComment(text) if text.trim() == "hi"));
    }

    #[test]
    fn block_comment_between_tokens() {
        let s = "a /* note */ b";
        let t = scan_str(s);
        assert_eq!(t.len(), 1);
        assert!(matches!(&t[0].kind, TriviaKind::BlockComment(text) if text.trim() == "note"));
    }

    #[test]
    fn blank_line_between_tokens() {
        let s = "a\n\nb";
        let t = scan_str(s);
        assert_eq!(t.len(), 1);
        assert!(matches!(t[0].kind, TriviaKind::BlankLine));
    }

    #[test]
    fn comment_inside_paren_group() {
        let s = "f(/* note */ x)";
        let t = scan_str(s);
        // The comment lives between the `(` open and the `x` ident.
        assert_eq!(t.len(), 1);
        assert!(matches!(&t[0].kind, TriviaKind::BlockComment(text) if text.trim() == "note"));
    }

    #[test]
    fn no_trivia_for_clean_source() {
        let s = "VStack { Button(\"ok\") }";
        let t = scan_str(s);
        assert!(t.is_empty());
    }
}
