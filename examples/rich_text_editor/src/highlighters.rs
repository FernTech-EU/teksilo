// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Three `SyntaxHighlighter` implementations showcased by the editor's
//! highlighter toolbar row.
//!
//! A highlighter lives on the `TextDocument` (not the widget) and produces
//! *shadow* [`HighlightFormat`] runs that overlay the real formatting at layout
//! time — they never mutate stored data, never show up in undo/export, and are
//! shared automatically by every editor bound to the same document. Each
//! highlighter here works purely on a block's plain text and reports
//! **character** offsets (not bytes).

use teksilo::text_document::{
    Color, HighlightContext, HighlightFormat, SyntaxHighlighter, UnderlineStyle,
};

/// A tiny hand lexer that colors Rust-ish keywords, integer literals and
/// double-quoted string literals — the canonical `QSyntaxHighlighter` shape.
///
/// It scans every block's plain text, so it tints tokens document-wide rather
/// than only inside code blocks; that keeps the demo simple and is plenty to
/// show the mechanism against the sample document's Rust/Python snippets.
pub struct KeywordHighlighter;

const KEYWORDS: &[&str] = &[
    "fn", "let", "mut", "pub", "struct", "enum", "impl", "use", "match", "if", "else", "for",
    "while", "loop", "return", "self", "mod", "trait", "const", "static", "as", "in", "where",
    "def", "class", "import", "from", "lambda", "None", "True", "False",
];

impl SyntaxHighlighter for KeywordHighlighter {
    fn highlight_block(&self, text: &str, ctx: &mut HighlightContext) {
        let chars: Vec<char> = text.chars().collect();
        let len = chars.len();
        let mut i = 0;
        while i < len {
            let c = chars[i];
            if c == '"' {
                // String literal: span to the closing quote (or end of block).
                let start = i;
                i += 1;
                while i < len && chars[i] != '"' {
                    i += 1;
                }
                if i < len {
                    i += 1; // include closing quote
                }
                ctx.set_format(start, i - start, fg(Color::rgb(60, 150, 90)));
            } else if c.is_ascii_digit() {
                let start = i;
                while i < len && (chars[i].is_ascii_digit() || chars[i] == '_' || chars[i] == '.') {
                    i += 1;
                }
                ctx.set_format(start, i - start, fg(Color::rgb(220, 130, 40)));
            } else if c.is_alphabetic() || c == '_' {
                let start = i;
                while i < len && (chars[i].is_alphanumeric() || chars[i] == '_') {
                    i += 1;
                }
                let word: String = chars[start..i].iter().collect();
                if KEYWORDS.contains(&word.as_str()) {
                    ctx.set_format(
                        start,
                        i - start,
                        HighlightFormat {
                            foreground_color: Some(Color::rgb(170, 90, 200)),
                            font_bold: Some(true),
                            ..Default::default()
                        },
                    );
                }
            } else {
                i += 1;
            }
        }
    }
}

/// Underlines a small set of commonly misspelled words with a red wavy
/// spell-check underline — shows underline-style shadow formatting.
pub struct SpellCheckHighlighter;

const MISSPELLED: &[&str] = &[
    "recieve",
    "teh",
    "seperate",
    "occured",
    "definately",
    "wich",
    "untill",
    "alot",
];

impl SyntaxHighlighter for SpellCheckHighlighter {
    fn highlight_block(&self, text: &str, ctx: &mut HighlightContext) {
        let chars: Vec<char> = text.chars().collect();
        let len = chars.len();
        let mut i = 0;
        while i < len {
            if chars[i].is_alphabetic() {
                let start = i;
                while i < len && (chars[i].is_alphanumeric() || chars[i] == '\'') {
                    i += 1;
                }
                let word: String = chars[start..i]
                    .iter()
                    .flat_map(|c| c.to_lowercase())
                    .collect();
                if MISSPELLED.contains(&word.as_str()) {
                    ctx.set_format(
                        start,
                        i - start,
                        HighlightFormat {
                            underline_style: Some(UnderlineStyle::SpellCheckUnderline),
                            underline_color: Some(Color::rgb(220, 40, 40)),
                            ..Default::default()
                        },
                    );
                }
            } else {
                i += 1;
            }
        }
    }
}

fn fg(color: Color) -> HighlightFormat {
    HighlightFormat {
        foreground_color: Some(color),
        ..Default::default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn spans(hl: &dyn SyntaxHighlighter, text: &str) -> Vec<teksilo::text_document::HighlightSpan> {
        let mut ctx = HighlightContext::new(0, -1, None);
        hl.highlight_block(text, &mut ctx);
        let (spans, _, _) = ctx.into_parts();
        spans
    }

    #[test]
    fn keyword_colors_keyword_number_and_string() {
        let s = spans(&KeywordHighlighter, "let x = 42; foo(\"hi\")");
        // `let` keyword, `42` number, `"hi"` string.
        assert_eq!(s.len(), 3);
        assert_eq!(s[0].format.foreground_color, Some(Color::rgb(170, 90, 200)));
        assert_eq!(s[0].format.font_bold, Some(true));
        assert_eq!(s[1].format.foreground_color, Some(Color::rgb(220, 130, 40)));
        assert_eq!(s[2].format.foreground_color, Some(Color::rgb(60, 150, 90)));
    }

    #[test]
    fn spellcheck_underlines_misspelled_words() {
        let s = spans(&SpellCheckHighlighter, "I will recieve teh package");
        assert_eq!(s.len(), 2);
        assert_eq!(
            s[0].format.underline_style,
            Some(UnderlineStyle::SpellCheckUnderline)
        );
        assert_eq!(s[0].format.underline_color, Some(Color::rgb(220, 40, 40)));
    }
}
