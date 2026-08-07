// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! `CodeEditor` demo — a source editor with everything injected, no language
//! baked in.
//!
//! Run with: `cargo run -p code_editor`.
//!
//! Everything the editor does that *looks* language-specific is a value this
//! example supplies — the comment token, the bracket pairs, the highlighter, the
//! completion candidates. The editor knows how to toggle a comment; it does not
//! know that this is Rust.
//!
//! Try, in the editor:
//!
//! - **Auto-close & match brackets** — type `(`, `[`, `{`; the caret's bracket
//!   and its partner get a faint wash.
//! - **Ctrl+/** — toggle a line comment on the caret's line or the selection.
//! - **Tab / Shift+Tab** — indent / dedent (a selection indents every line).
//! - **Ctrl+D** — add a caret on the next line; type into all of them at once.
//! - **Alt+↑ / Alt+↓** — move the current line up / down.
//! - **Ctrl+Space** (or just type) — completion: the injected provider offers
//!   keywords, the editor filters by the word under the caret.
//! - The gutter numbers every line; the caret's line gets a background band.
//!
//! The status bar reflects the live caret position and caret count.

use std::sync::Arc;

use teksilo::core::WidgetPlacement;
use teksilo::prelude::*;
use teksilo::text_document::{
    Color, HighlightContext, HighlightFormat, SyntaxHighlighter, TextDocument,
};
use teksilo::widgets::{
    COMMON_BRACKETS, CodeEditor, CodeEditorHandle, CompletionContext, CompletionItem,
    CompletionKind, Expand, HStack, TextWidget, ThemeSwitcher, Toolbar,
};

/// The one list of "words this language knows" — shared by the highlighter and
/// the completion provider, so both stay in step. In a real editor this comes
/// from a grammar or a language server; here it is a constant.
const KEYWORDS: &[&str] = &[
    "fn",
    "let",
    "mut",
    "pub",
    "struct",
    "enum",
    "impl",
    "use",
    "match",
    "if",
    "else",
    "for",
    "while",
    "loop",
    "return",
    "self",
    "mod",
    "trait",
    "const",
    "static",
    "as",
    "in",
    "where",
    "Widget",
    "Signal",
    "Vec",
    "BuildContext",
    "WidgetId",
    "TextWidget",
    "Button",
];

const SAMPLE: &str = "// A little Teksilo widget. Edit me!
// Ctrl+/ comments a line, Tab indents, Ctrl+D adds a caret.
struct Counter {
    count: Signal<i32>,
}

impl Widget for Counter {
    fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
        let label = TextWidget::new(lit!(\"Count\"));
        let button = Button::new(lit!(\"Increment\")).on_activate_fn(|ctx| {
            // Type '(' or '{' here \u{2014} the closing partner appears.
        });
        vec![ctx.add(label), ctx.add(button)]
    }
}
";

/// A shadow-format highlighter: it overlays colours at layout time, never
/// mutating the document. Colours keywords purple, strings orange, and line
/// comments grey — a hand lexer standing in for a grammar.
struct CodeHighlighter;

fn fg(color: Color) -> HighlightFormat {
    HighlightFormat {
        foreground_color: Some(color),
        ..Default::default()
    }
}

impl SyntaxHighlighter for CodeHighlighter {
    fn highlight_block(&self, text: &str, ctx: &mut HighlightContext) {
        let chars: Vec<char> = text.chars().collect();
        let n = chars.len();
        let mut i = 0;
        while i < n {
            let c = chars[i];
            if c == '/' && i + 1 < n && chars[i + 1] == '/' {
                ctx.set_format(i, n - i, fg(Color::rgb(120, 132, 145)));
                break;
            } else if c == '"' {
                let start = i;
                i += 1;
                while i < n && chars[i] != '"' {
                    i += 1;
                }
                if i < n {
                    i += 1; // closing quote
                }
                ctx.set_format(start, i - start, fg(Color::rgb(205, 145, 75)));
            } else if c.is_alphabetic() || c == '_' {
                let start = i;
                while i < n && (chars[i].is_alphanumeric() || chars[i] == '_') {
                    i += 1;
                }
                let word: String = chars[start..i].iter().collect();
                if KEYWORDS.contains(&word.as_str()) {
                    ctx.set_format(start, i - start, fg(Color::rgb(178, 108, 210)));
                }
            } else {
                i += 1;
            }
        }
    }
}

/// The completion provider. It returns the keyword set; the editor filters it by
/// the word under the caret and shows the popup. Language-agnostic: the app
/// knows the candidates, the editor knows the mechanics.
fn complete(_ctx: &CompletionContext) -> Vec<CompletionItem> {
    KEYWORDS
        .iter()
        .map(|k| {
            CompletionItem::new(*k)
                .kind(CompletionKind::Keyword)
                .detail("keyword")
        })
        .collect()
}

/// The demo root: the editor plus a live status bar.
struct EditorDemo {
    editor: Option<CodeEditor>,
    handle: CodeEditorHandle,
    editor_id: Option<WidgetId>,
    toolbar_id: Option<WidgetId>,
}

impl std::fmt::Debug for EditorDemo {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("EditorDemo").finish_non_exhaustive()
    }
}

impl EditorDemo {
    fn new() -> Self {
        let doc = TextDocument::new();
        doc.set_plain_text(SAMPLE).expect("seed the sample");
        // The highlighter lives on the document, shared by every view of it.
        doc.add_syntax_session(Arc::new(CodeHighlighter));

        let editor = CodeEditor::new(doc)
            .font_family("monospace")
            .line_comment("//")
            .bracket_pairs(COMMON_BRACKETS.to_vec())
            .auto_close_brackets(true)
            .bracket_matching(true)
            .completion_provider(complete);
        let handle = editor.handle();

        Self {
            editor: Some(editor),
            handle,
            editor_id: None,
            toolbar_id: None,
        }
    }

    fn status_bar(&self, ctx: &mut BuildContext) -> WidgetId {
        let status = {
            let pos = self.handle.cursor_position_signal();
            let carets = self.handle.caret_count();
            let sel = self.handle.has_selection();
            pos.zip3(&carets, &sel).map(|(p, c, s)| {
                let carets = if *c == 1 {
                    "1 caret".to_string()
                } else {
                    format!("{c} carets")
                };
                let sel = if *s { " \u{00B7} selection" } else { "" };
                format!("char {p} \u{00B7} {carets}{sel}")
            })
        };

        ctx.add(
            Toolbar::new().child(
                HStack::new()
                    .spacing(8.0)
                    .child(TextWidget::new(lit!("CodeEditor")).style(TextStyleRole::BodyBold))
                    .child(
                        Expand::new().child(
                            TextWidget::new(lit!(""))
                                .text(status)
                                .style(TextStyleRole::Small),
                        ),
                    )
                    .child(ThemeSwitcher::new()),
            ),
        )
    }
}

impl Widget for EditorDemo {
    fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
        let toolbar = self.status_bar(ctx);
        self.toolbar_id = Some(toolbar);

        let editor = self.editor.take().expect("EditorDemo built once");
        let editor_id = ctx.add(editor);
        self.editor_id = Some(editor_id);

        vec![toolbar, editor_id]
    }

    fn layout_response(
        &self,
        proposal: SizeProposal,
        _ctx: &LayoutContext,
    ) -> teksilo::core::widget::LayoutResponse {
        Size::new(
            proposal.width.unwrap_or(900.0),
            proposal.height.unwrap_or(600.0),
        )
        .into()
    }

    fn place_children(
        &self,
        bounds: Rect,
        _proposal: SizeProposal,
        children: &mut [WidgetPlacement],
        ctx: &LayoutContext,
    ) {
        let toolbar_h = self
            .toolbar_id
            .and_then(|id| ctx.child_size(id, SizeProposal::with_width(bounds.width)))
            .map(|s| s.height)
            .unwrap_or(44.0);
        for child in children.iter_mut() {
            if Some(child.id) == self.toolbar_id {
                child.origin = Point::new(bounds.x, bounds.y);
                child.size = Size::new(bounds.width, toolbar_h);
            } else if Some(child.id) == self.editor_id {
                child.origin = Point::new(bounds.x, bounds.y + toolbar_h);
                child.size = Size::new(bounds.width, (bounds.height - toolbar_h).max(0.0));
            }
        }
    }

    fn children(&self) -> Vec<WidgetId> {
        let mut ids = Vec::new();
        ids.extend(self.toolbar_id);
        ids.extend(self.editor_id);
        ids
    }
}

fn main() {
    TeksiloAppBuilder::new()
        .install_inspector_in_debug()
        .theme(teksilo::presets::intui::dark())
        .initial_window(
            WindowConfig::new()
                .title("Teksilo \u{2014} CodeEditor")
                .size(900, 640)
                .root(|tree, _state| tree.add(EditorDemo::new())),
        )
        .run();
}
