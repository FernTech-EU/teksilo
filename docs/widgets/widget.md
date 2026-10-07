<!-- SPDX-License-Identifier: MPL-2.0 -->
<!-- SPDX-FileCopyrightText: 2026 FernTech -->

# CodeEditor

The public editing surfaces: `CodeEditor` and `PlainTextEditor`.

## Public types

| Kind | Name |
| ---: | :--- |
| `struct` | [`CodeEditor`](#codeeditor) — A multi-line source-code editing surface: gutter, current-line highlight, indentation, bracket handling, and multiple carets |
| `struct` | [`PlainTextEditor`](#plaintexteditor) — A multi-line plain-text editing surface — the code editor with its code affordances off and wrapping on |

## Public functions

### `CodeEditor`

| Returns | Function |
| ---: | :--- |
| | **Constructors** |
| `Self` | [`new(document: TextDocument)`](#codeeditor-new) |
| `Self` | [`read_only(document: TextDocument)`](#codeeditor-read_only) |
| | **Builder methods** |
| `Self` | [`label(label: impl Into<teksilo_i18n::LocalizedString>)`](#codeeditor-label) |
| `Self` | [`context_menu(factory: impl Fn( teksilo_canvas::Point, &mut teksilo_core::widget::EventContext, ) -> Option<Box<dyn teksilo_core::widget::Widget>> + 'static)`](#codeeditor-context_menu) |
| `Self` | [`default_context_menu(enabled: bool)`](#codeeditor-default_context_menu) |
| `Self` | [`wrap_mode(mode: WrapMode)`](#codeeditor-wrap_mode) |
| `Self` | [`v_scroll_policy(policy: ScrollPolicy)`](#codeeditor-v_scroll_policy) |
| `Self` | [`h_scroll_policy(policy: ScrollPolicy)`](#codeeditor-h_scroll_policy) |
| `Self` | [`overscroll_behavior(behavior: OverscrollBehavior)`](#codeeditor-overscroll_behavior) |
| `Self` | [`window_to_clip(on: bool)`](#codeeditor-window_to_clip) |
| `Self` | [`min_lines(lines: u32)`](#codeeditor-min_lines) |
| `Self` | [`max_lines(lines: u32)`](#codeeditor-max_lines) |
| `Self` | [`font_family(family: impl Into<String>)`](#codeeditor-font_family) |
| `Self` | [`font_size_scale(scale: f32)`](#codeeditor-font_size_scale) |
| `Self` | [`follow_text_scale(follow: bool)`](#codeeditor-follow_text_scale) |
| `Self` | [`on_change(callback: impl Fn() + 'static)`](#codeeditor-on_change) |
| `Self` | [`background(color: impl Into<teksilo_core::color_prop::ColorProp>)`](#codeeditor-background) |
| `Self` | [`text_color(color: impl Into<teksilo_core::color_prop::ColorProp>)`](#codeeditor-text_color) |
| `Self` | [`caret_color(color: impl Into<teksilo_core::color_prop::ColorProp>)`](#codeeditor-caret_color) |
| `Self` | [`selection_color(color: impl Into<teksilo_core::color_prop::ColorProp>)`](#codeeditor-selection_color) |
| `Self` | [`gutter(show: bool)`](#codeeditor-gutter) |
| `Self` | [`current_line_highlight(on: bool)`](#codeeditor-current_line_highlight) |
| `Self` | [`indent_style(style: IndentStyle)`](#codeeditor-indent_style) |
| `Self` | [`tab_width(width: u8)`](#codeeditor-tab_width) |
| `Self` | [`use_soft_tabs(soft: bool)`](#codeeditor-use_soft_tabs) |
| `Self` | [`auto_indent(on: bool)`](#codeeditor-auto_indent) |
| `Self` | [`bracket_pairs(pairs: impl Into<Vec<BracketPair>>)`](#codeeditor-bracket_pairs) |
| `Self` | [`auto_close_brackets(on: bool)`](#codeeditor-auto_close_brackets) |
| `Self` | [`bracket_matching(on: bool)`](#codeeditor-bracket_matching) |
| `Self` | [`line_comment(token: impl Into<String>)`](#codeeditor-line_comment) |
| `Self` | [`completion_provider(provider: impl Fn(&CompletionContext) -> Vec<CompletionItem> + 'static)`](#codeeditor-completion_provider) |
| `Self` | [`auto_complete(auto: bool)`](#codeeditor-auto_complete) |
| | **Methods** |
| `CodeEditorHandle` | [`handle()`](#codeeditor-handle) |

### `PlainTextEditor`

| Returns | Function |
| ---: | :--- |
| | **Constructors** |
| `Self` | [`new(document: TextDocument)`](#plaintexteditor-new) |
| `Self` | [`read_only(document: TextDocument)`](#plaintexteditor-read_only) |
| | **Builder methods** |
| `Self` | [`min_lines(lines: u32)`](#plaintexteditor-min_lines) |
| `Self` | [`max_lines(lines: u32)`](#plaintexteditor-max_lines) |
| `Self` | [`wrap_mode(mode: WrapMode)`](#plaintexteditor-wrap_mode) |
| `Self` | [`font_family(family: impl Into<String>)`](#plaintexteditor-font_family) |
| `Self` | [`follow_text_scale(follow: bool)`](#plaintexteditor-follow_text_scale) |
| `Self` | [`font_size_scale(scale: f32)`](#plaintexteditor-font_size_scale) |
| `Self` | [`on_change(callback: impl Fn() + 'static)`](#plaintexteditor-on_change) |
| `Self` | [`background(color: impl Into<teksilo_core::color_prop::ColorProp>)`](#plaintexteditor-background) |
| `Self` | [`context_menu(factory: impl Fn( teksilo_canvas::Point, &mut teksilo_core::widget::EventContext, ) -> Option<Box<dyn teksilo_core::widget::Widget>> + 'static)`](#plaintexteditor-context_menu) |
| `Self` | [`default_context_menu(enabled: bool)`](#plaintexteditor-default_context_menu) |
| `Self` | [`label(label: impl Into<teksilo_i18n::LocalizedString>)`](#plaintexteditor-label) |
| | **Methods** |
| `CodeEditorHandle` | [`handle()`](#plaintexteditor-handle) |

## Detailed description

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

#### Pan to scroll

The surface installs `common::scrollable::ScrollableBehavior`
— the shared wheel arithmetic, a finger's pan, and the `PanClaim`. The wheel
path is unchanged: no tween (these offsets are plain signals), 16 dp a line,
`Ignored` at a hard boundary so the page around it takes the rest, and a
repaint asked for exactly when an axis moved.

**The claim serves this surface even though it also owns the press
arena**, which its double- and triple-tap recognizers give it. The router
stops its arbitration walk at the press owner only for a `Gesture` member,
whose recognizer the capture dispatch is already driving; a `Pan` member is
decided in that walk and nowhere else, so it is exempt. A finger on the
text therefore scrolls the text, and hands the gesture outward only at this
surface's own boundary. See `docs/kinetic-scrolling.md` §10.1.

## API reference

📖 [Full rustdoc API for this module](https://docs.rs/teksilo-widgets/latest/teksilo_widgets/code_editor/index.html)

<a id="codeeditor"></a>

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

<a id="codeeditor-new"></a>

#### `pub fn new(document: TextDocument) -> Self`

An editable code editor bound to `document`: gutter on, current-line
highlight on, no wrapping. Code affordances (comment token, bracket
pairs) stay off until the application supplies them — the editor never
guesses a language.

<a id="codeeditor-read_only"></a>

#### `pub fn read_only(document: TextDocument) -> Self`

A read-only code viewer bound to `document`: no caret, navigation and
copy only, `Role::Document`. Still gets the gutter and syntax colours.

<a id="codeeditor-label"></a>

#### `pub fn label(mut self, label: impl Into<teksilo_i18n::LocalizedString>) -> Self`

Accessible name for the editor.

Applied to the body that holds the text, which is the node focus is
published on and the one a screen reader announces ("Code, entry"). The
editor's own node is structure that no adapter shows. An
`.access_label(..)`, `.access_labelled_by(..)` or tooltip attached to
the editor reaches the same node.

Stays locale-reactive: a `tr!(...)` name is re-resolved when the
locale changes, without a rebuild.

<a id="codeeditor-context_menu"></a>

#### `pub fn context_menu( mut self, factory: impl Fn( teksilo_canvas::Point, &mut teksilo_core::widget::EventContext, ) -> Option<Box<dyn teksilo_core::widget::Widget>> + 'static, ) -> Self`

Replace the built-in right-click menu with `factory`, called on each
right-click with the **window** position of the click. Returning `None`
shows no menu.

A replacement is responsible for repositioning the caret if it wants the
platform convention — the built-in menu does it through
`context_menu::factory`, and only when a pointer opened the menu
(`EventContext::context_menu_trigger`).
Shift+F10 hands the factory an anchor, not a place the user chose.

<a id="codeeditor-default_context_menu"></a>

#### `pub fn default_context_menu(mut self, enabled: bool) -> Self`

Whether to install the built-in Cut / Copy / Paste / Select All menu
(default `true`). `false` lets a right-click bubble past the editor, so an
application can render its own menu from outside; a factory installed with
`context_menu` wins over this either way.

The **touch** selection toolbar is *not* affected. It is raised by the
controller rather than by a right-click, its rows are the same four
commands, and a surface with no menu still has to be usable by a finger —
which has no second button and no chord.

<a id="codeeditor-wrap_mode"></a>

#### `pub fn wrap_mode(self, mode: WrapMode) -> Self`

Set the line-wrap mode. `CodeEditor` defaults to `WrapMode::None` (source
lines must not fold, or the gutter's one-number-per-line correspondence
breaks); pair with `.h_scroll_policy(Auto)` to scroll wide lines.

<a id="codeeditor-v_scroll_policy"></a>

#### `pub fn v_scroll_policy(mut self, policy: ScrollPolicy) -> Self`

Vertical scrollbar policy (default `Auto`).

<a id="codeeditor-h_scroll_policy"></a>

#### `pub fn h_scroll_policy(mut self, policy: ScrollPolicy) -> Self`

Horizontal scrollbar policy (default `Auto`).

<a id="codeeditor-overscroll_behavior"></a>

#### `pub fn overscroll_behavior(mut self, behavior: OverscrollBehavior) -> Self`

Wheel scroll-chaining at the editor's scroll boundary. `Chain` (default)
hands leftover scroll to an enclosing scrollable; `Contain` absorbs it.

<a id="codeeditor-window_to_clip"></a>

#### `pub fn window_to_clip(self, on: bool) -> Self`

Cull the render to the visible clip band (default `false`). Turn on only
for an editor deliberately laid out at full document height inside an
outer `ScrollArea` (`v_scroll_policy(AlwaysOff)` + `min_lines(1)`): the
body's bounds then span the whole document, and this renders only the
on-screen slice instead of every line. A normally-scrolling editor already
renders just a viewport's worth, so it needs nothing.

<a id="codeeditor-min_lines"></a>

#### `pub fn min_lines(mut self, lines: u32) -> Self`

Minimum visible height in lines — switches the editor from greedy (fill
the proposal) to intrinsic sizing (grow with content up to `max_lines`,
then scroll). The composer pattern.

<a id="codeeditor-max_lines"></a>

#### `pub fn max_lines(mut self, lines: u32) -> Self`

Maximum visible height in lines — caps intrinsic growth.

<a id="codeeditor-font_family"></a>

#### `pub fn font_family(self, family: impl Into<String>) -> Self`

Fallback font family for the document's text. `None` (the default) keeps
the typesetter's registry default; a code editor should pass a monospace
family so columns line up.

<a id="codeeditor-font_size_scale"></a>

#### `pub fn font_size_scale(self, scale: f32) -> Self`

Per-editor logical font-size multiplier (`1.0` = 100 %), composed with
the accessibility text scale when `follow_text_scale`
is on. Sharp — shapes at a larger ppem.

<a id="codeeditor-follow_text_scale"></a>

#### `pub fn follow_text_scale(self, follow: bool) -> Self`

Whether the editor grows text with the global accessibility text scale
(default `true`). Turn off for a WYSIWYG surface whose font sizes are
document content. Composed with `font_size_scale`.

<a id="codeeditor-on_change"></a>

#### `pub fn on_change(self, callback: impl Fn() + 'static) -> Self`

A callback fired once per drain batch that contained a real content edit.

<a id="codeeditor-background"></a>

#### `pub fn background(self, color: impl Into<teksilo_core::color_prop::ColorProp>) -> Self`

Override the editor background colour (accepts `Color`, a theme role, or a
`Signal`). `None`-equivalent default tracks the theme's `editor_bg`.

<a id="codeeditor-text_color"></a>

#### `pub fn text_color(self, color: impl Into<teksilo_core::color_prop::ColorProp>) -> Self`

Override the text colour. Default tracks the theme's `editor_fg`.

<a id="codeeditor-caret_color"></a>

#### `pub fn caret_color(self, color: impl Into<teksilo_core::color_prop::ColorProp>) -> Self`

Override the caret colour. Default tracks the theme's `editor_caret`.

<a id="codeeditor-selection_color"></a>

#### `pub fn selection_color(self, color: impl Into<teksilo_core::color_prop::ColorProp>) -> Self`

Override the selection colour. A pinned colour opts out of the
window-inactive desaturation.

<a id="codeeditor-gutter"></a>

#### `pub fn gutter(mut self, show: bool) -> Self`

Whether the line-number gutter is shown (default `true`).

<a id="codeeditor-current_line_highlight"></a>

#### `pub fn current_line_highlight(self, on: bool) -> Self`

Whether the caret's line gets a full-width background wash (default
`true` for `CodeEditor`).

<a id="codeeditor-indent_style"></a>

#### `pub fn indent_style(self, style: IndentStyle) -> Self`

Set the indentation style directly (spaces of a width, or tabs rendered a
width wide).

<a id="codeeditor-tab_width"></a>

#### `pub fn tab_width(self, width: u8) -> Self`

Set the indent width, keeping the current spaces-vs-tabs kind.

<a id="codeeditor-use_soft_tabs"></a>

#### `pub fn use_soft_tabs(self, soft: bool) -> Self`

Whether indentation is written with spaces (`true`, the default) or a tab
character (`false`), keeping the current width.

<a id="codeeditor-auto_indent"></a>

#### `pub fn auto_indent(self, on: bool) -> Self`

Whether Enter carries the current line's indentation onto the new line
(default `true`).

<a id="codeeditor-bracket_pairs"></a>

#### `pub fn bracket_pairs(self, pairs: impl Into<Vec<BracketPair>>) -> Self`

The delimiter pairs the editor auto-closes and match-highlights. Empty
(the default) disables both.

<a id="codeeditor-auto_close_brackets"></a>

#### `pub fn auto_close_brackets(self, on: bool) -> Self`

Whether typing an opener inserts its closing partner (default `false`;
needs configured `bracket_pairs`).

<a id="codeeditor-bracket_matching"></a>

#### `pub fn bracket_matching(self, on: bool) -> Self`

Whether the delimiter matching the caret's is highlighted (default
`false`; needs configured `bracket_pairs`).

<a id="codeeditor-line_comment"></a>

#### `pub fn line_comment(self, token: impl Into<String>) -> Self`

The token that starts a line comment (`"//"`, `"#"`, `"--"`). Enables
`Ctrl+/` comment toggling; unset (the default) leaves it a no-op rather
than guessing.

<a id="codeeditor-completion_provider"></a>

#### `pub fn completion_provider( self, provider: impl Fn(&CompletionContext) -> Vec<CompletionItem> + 'static, ) -> Self`

Supply the completion candidates. The provider is called for the word
being completed and given a `CompletionContext`; the editor filters its
result by the live prefix, shows the popup, and replaces the word on
accept. Language-agnostic — the app knows the candidates, the editor knows
the mechanics. Without a provider there is no completion.

<a id="codeeditor-auto_complete"></a>

#### `pub fn auto_complete(self, auto: bool) -> Self`

Whether typing an identifier character opens the completion popup
automatically (default `true`). When off, only `Ctrl+Space` opens it.

<a id="codeeditor-handle"></a>

#### `pub fn handle(&self) -> CodeEditorHandle`

A cloneable handle to drive the editor from a toolbar, shortcut, or test.

<a id="plaintexteditor"></a>

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

<a id="plaintexteditor-new"></a>

#### `pub fn new(document: TextDocument) -> Self`

An editable plain-text editor bound to `document`: no gutter, no
current-line highlight, word wrapping, and no code affordances.

<a id="plaintexteditor-read_only"></a>

#### `pub fn read_only(document: TextDocument) -> Self`

A read-only plain-text viewer bound to `document`.

<a id="plaintexteditor-min_lines"></a>

#### `pub fn min_lines(mut self, lines: u32) -> Self`

Restrict growth to `[min, max]` lines (intrinsic sizing — the composer
pattern).

<a id="plaintexteditor-max_lines"></a>

#### `pub fn max_lines(mut self, lines: u32) -> Self`

Cap intrinsic growth at `lines`.

<a id="plaintexteditor-wrap_mode"></a>

#### `pub fn wrap_mode(mut self, mode: WrapMode) -> Self`

Set the line-wrap mode (default `Word`).

<a id="plaintexteditor-font_family"></a>

#### `pub fn font_family(mut self, family: impl Into<String>) -> Self`

Fallback font family.

<a id="plaintexteditor-follow_text_scale"></a>

#### `pub fn follow_text_scale(mut self, follow: bool) -> Self`

Whether the editor follows the global accessibility text scale.

<a id="plaintexteditor-font_size_scale"></a>

#### `pub fn font_size_scale(mut self, scale: f32) -> Self`

Per-editor logical font-size multiplier (`1.0` = 100 %).

<a id="plaintexteditor-on_change"></a>

#### `pub fn on_change(mut self, callback: impl Fn() + 'static) -> Self`

A callback fired on each content-changing edit batch.

<a id="plaintexteditor-background"></a>

#### `pub fn background(mut self, color: impl Into<teksilo_core::color_prop::ColorProp>) -> Self`

Override the background colour.

<a id="plaintexteditor-context_menu"></a>

#### `pub fn context_menu( mut self, factory: impl Fn( teksilo_canvas::Point, &mut teksilo_core::widget::EventContext, ) -> Option<Box<dyn teksilo_core::widget::Widget>> + 'static, ) -> Self`

Replace the built-in right-click menu — see
`CodeEditor::context_menu`.

<a id="plaintexteditor-default_context_menu"></a>

#### `pub fn default_context_menu(mut self, enabled: bool) -> Self`

Whether to install the built-in right-click menu — see
`CodeEditor::default_context_menu`.

<a id="plaintexteditor-label"></a>

#### `pub fn label(mut self, label: impl Into<teksilo_i18n::LocalizedString>) -> Self`

Accessible name for the editor: see `CodeEditor::label`.

<a id="plaintexteditor-handle"></a>

#### `pub fn handle(&self) -> CodeEditorHandle`

A cloneable handle to drive the editor.
