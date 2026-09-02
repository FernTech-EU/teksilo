<!-- SPDX-License-Identifier: MPL-2.0 -->
<!-- SPDX-FileCopyrightText: 2026 FernTech -->

# CodeEditor

The public editing surfaces: `CodeEditor` and `PlainTextEditor`.

The wrapper is the focus + event target; it owns the gutter (optional), the
paint-only body, and the overlay scrollbars, joined to them only through the
shared `CodeEditorState`. This mirrors
`RichTextEditor` exactly — the wrapper carries focus so a future style may
place the body anywhere in its chrome without the focus semantics moving —
and adds the two things a source editor needs on top: a line-number gutter to
the left, and a paint pass that draws the current-line band (across gutter and
body) and the matched-bracket cells behind the text.

`PlainTextEditor` is the same machinery with the code affordances off and
wrapping on — a notes field, a commit message — so the two never drift.

## Builder methods at a glance

`read_only`, `wrap_mode`, `v_scroll_policy`, `h_scroll_policy`, `overscroll_behavior`, `window_to_clip`, `min_lines`, `max_lines`, `font_family`, `font_size_scale`, `follow_text_scale`, `on_change`, `background`, `text_color`, `caret_color`, `selection_color`, `gutter`, `current_line_highlight`, `indent_style`, `tab_width`, `use_soft_tabs`, `auto_indent`, `bracket_pairs`, `auto_close_brackets`, `bracket_matching`, `line_comment`, `completion_provider`, `auto_complete`, `handle`

## API reference

📖 [Full rustdoc API for this module](https://docs.rs/teksilo-widgets/latest/teksilo_widgets/code_editor/index.html)

## `pub struct CodeEditor`

A multi-line source-code editing surface: gutter, current-line highlight,
indentation, bracket handling, and multiple carets.

Construct with `CodeEditor::new` (editable) or `CodeEditor::read_only`
(view + select + copy). Every code affordance is injected configuration, not
a built-in language — see `CodeConfig`.

```rust
pub struct CodeEditor { /* fields */ }
```

### Methods

#### `pub fn new(document: TextDocument) -> Self`

An editable code editor bound to `document`: gutter on, current-line
highlight on, no wrapping. Code affordances (comment token, bracket
pairs) stay off until the application supplies them — the editor never
guesses a language.

#### `pub fn read_only(document: TextDocument) -> Self`

A read-only code viewer bound to `document`: no caret, navigation and
copy only, `Role::Document`. Still gets the gutter and syntax colours.

#### `pub fn wrap_mode(self, mode: WrapMode) -> Self`

Set the line-wrap mode. `CodeEditor` defaults to `WrapMode::None` (source
lines must not fold, or the gutter's one-number-per-line correspondence
breaks); pair with `.h_scroll_policy(Auto)` to scroll wide lines.

#### `pub fn v_scroll_policy(mut self, policy: ScrollPolicy) -> Self`

Vertical scrollbar policy (default `Auto`).

#### `pub fn h_scroll_policy(mut self, policy: ScrollPolicy) -> Self`

Horizontal scrollbar policy (default `Auto`).

#### `pub fn overscroll_behavior(mut self, behavior: OverscrollBehavior) -> Self`

Wheel scroll-chaining at the editor's scroll boundary. `Chain` (default)
hands leftover scroll to an enclosing scrollable; `Contain` absorbs it.

#### `pub fn window_to_clip(self, on: bool) -> Self`

Cull the render to the visible clip band (default `false`). Turn on only
for an editor deliberately laid out at full document height inside an
outer `ScrollArea` (`v_scroll_policy(AlwaysOff)` + `min_lines(1)`): the
body's bounds then span the whole document, and this renders only the
on-screen slice instead of every line. A normally-scrolling editor already
renders just a viewport's worth, so it needs nothing.

#### `pub fn min_lines(mut self, lines: u32) -> Self`

Minimum visible height in lines — switches the editor from greedy (fill
the proposal) to intrinsic sizing (grow with content up to `max_lines`,
then scroll). The composer pattern.

#### `pub fn max_lines(mut self, lines: u32) -> Self`

Maximum visible height in lines — caps intrinsic growth.

#### `pub fn font_family(self, family: impl Into<String>) -> Self`

Fallback font family for the document's text. `None` (the default) keeps
the typesetter's registry default; a code editor should pass a monospace
family so columns line up.

#### `pub fn font_size_scale(self, scale: f32) -> Self`

Per-editor logical font-size multiplier (`1.0` = 100 %), composed with
the accessibility text scale when `follow_text_scale`
is on. Sharp — shapes at a larger ppem.

#### `pub fn follow_text_scale(self, follow: bool) -> Self`

Whether the editor grows text with the global accessibility text scale
(default `true`). Turn off for a WYSIWYG surface whose font sizes are
document content. Composed with `font_size_scale`.

#### `pub fn on_change(self, callback: impl Fn() + 'static) -> Self`

A callback fired once per drain batch that contained a real content edit.

#### `pub fn background(self, color: impl Into<teksilo_core::color_prop::ColorProp>) -> Self`

Override the editor background colour (accepts `Color`, a theme role, or a
`Signal`). `None`-equivalent default tracks the theme's `editor_bg`.

#### `pub fn text_color(self, color: impl Into<teksilo_core::color_prop::ColorProp>) -> Self`

Override the text colour. Default tracks the theme's `editor_fg`.

#### `pub fn caret_color(self, color: impl Into<teksilo_core::color_prop::ColorProp>) -> Self`

Override the caret colour. Default tracks the theme's `editor_caret`.

#### `pub fn selection_color(self, color: impl Into<teksilo_core::color_prop::ColorProp>) -> Self`

Override the selection colour. A pinned colour opts out of the
window-inactive desaturation.

#### `pub fn gutter(mut self, show: bool) -> Self`

Whether the line-number gutter is shown (default `true`).

#### `pub fn current_line_highlight(self, on: bool) -> Self`

Whether the caret's line gets a full-width background wash (default
`true` for `CodeEditor`).

#### `pub fn indent_style(self, style: IndentStyle) -> Self`

Set the indentation style directly (spaces of a width, or tabs rendered a
width wide).

#### `pub fn tab_width(self, width: u8) -> Self`

Set the indent width, keeping the current spaces-vs-tabs kind.

#### `pub fn use_soft_tabs(self, soft: bool) -> Self`

Whether indentation is written with spaces (`true`, the default) or a tab
character (`false`), keeping the current width.

#### `pub fn auto_indent(self, on: bool) -> Self`

Whether Enter carries the current line's indentation onto the new line
(default `true`).

#### `pub fn bracket_pairs(self, pairs: impl Into<Vec<BracketPair>>) -> Self`

The delimiter pairs the editor auto-closes and match-highlights. Empty
(the default) disables both.

#### `pub fn auto_close_brackets(self, on: bool) -> Self`

Whether typing an opener inserts its closing partner (default `false`;
needs configured `bracket_pairs`).

#### `pub fn bracket_matching(self, on: bool) -> Self`

Whether the delimiter matching the caret's is highlighted (default
`false`; needs configured `bracket_pairs`).

#### `pub fn line_comment(self, token: impl Into<String>) -> Self`

The token that starts a line comment (`"//"`, `"#"`, `"--"`). Enables
`Ctrl+/` comment toggling; unset (the default) leaves it a no-op rather
than guessing.

#### `pub fn completion_provider( self, provider: impl Fn(&CompletionContext) -> Vec<CompletionItem> + 'static, ) -> Self`

Supply the completion candidates. The provider is called for the word
being completed and given a `CompletionContext`; the editor filters its
result by the live prefix, shows the popup, and replaces the word on
accept. Language-agnostic — the app knows the candidates, the editor knows
the mechanics. Without a provider there is no completion.

#### `pub fn auto_complete(self, auto: bool) -> Self`

Whether typing an identifier character opens the completion popup
automatically (default `true`). When off, only `Ctrl+Space` opens it.

#### `pub fn handle(&self) -> CodeEditorHandle`

A cloneable handle to drive the editor from a toolbar, shortcut, or test.

## `pub struct PlainTextEditor`

A multi-line plain-text editing surface — the code editor with its code
affordances off and wrapping on. A notes field, a commit message, a
description box.

It shares `CodeEditor`'s machinery (caret, selection, IME, clipboard,
scrolling, accessibility); the difference is configuration, so the two never
drift. Construct with `PlainTextEditor::new` / `PlainTextEditor::read_only`.

```rust
pub struct PlainTextEditor { /* fields */ }
```

### Methods

#### `pub fn new(document: TextDocument) -> Self`

An editable plain-text editor bound to `document`: no gutter, no
current-line highlight, word wrapping, and no code affordances.

#### `pub fn read_only(document: TextDocument) -> Self`

A read-only plain-text viewer bound to `document`.

#### `pub fn min_lines(mut self, lines: u32) -> Self`

Restrict growth to `[min, max]` lines (intrinsic sizing — the composer
pattern).

#### `pub fn max_lines(mut self, lines: u32) -> Self`

Cap intrinsic growth at `lines`.

#### `pub fn wrap_mode(mut self, mode: WrapMode) -> Self`

Set the line-wrap mode (default `Word`).

#### `pub fn font_family(mut self, family: impl Into<String>) -> Self`

Fallback font family.

#### `pub fn follow_text_scale(mut self, follow: bool) -> Self`

Whether the editor follows the global accessibility text scale.

#### `pub fn font_size_scale(mut self, scale: f32) -> Self`

Per-editor logical font-size multiplier (`1.0` = 100 %).

#### `pub fn on_change(mut self, callback: impl Fn() + 'static) -> Self`

A callback fired on each content-changing edit batch.

#### `pub fn background(mut self, color: impl Into<teksilo_core::color_prop::ColorProp>) -> Self`

Override the background colour.

#### `pub fn handle(&self) -> CodeEditorHandle`

A cloneable handle to drive the editor.
