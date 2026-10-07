<!-- SPDX-License-Identifier: MPL-2.0 -->
<!-- SPDX-FileCopyrightText: 2026 FernTech -->

# CodeEditorHandle

![CodeEditorHandle preview](img/code_editor.png)

Multi-line plain-text and code editing surfaces.

## Public types

| Kind | Name |
| ---: | :--- |
| `struct` | [`CodeEditorHandle`](#codeeditorhandle) — A handle onto a live editor, cloneable and detachable from the widget |
| `struct` | [`CompletionItem`](#completionitem) — A completion candidate |
| `enum` | [`CompletionKind`](#completionkind) — The category of a completion candidate — drives a small leading badge only |
| `struct` | [`CompletionContext`](#completioncontext) — What a completion provider is told about the caret when asked for candidates |
| `enum` | [`IndentStyle`](#indentstyle) — How a line's leading indentation is written |
| `struct` | [`BracketPair`](#bracketpair) — A pair of characters the editor treats as opening and closing delimiters |
| `const` | [`COMMON_BRACKETS`](#common_brackets) — The three pairs that are structural in essentially every bracketed language |
| `struct` | [`CodeConfig`](#codeconfig) — Editing behaviour the code editor applies, all supplied by the application |

## Public functions

### `CodeEditorHandle`

| Returns | Function |
| ---: | :--- |
| | **Methods** |
| `usize` | [`cursor_position()`](#codeeditorhandle-cursor_position) |
| `teksilo_core::Signal<usize>` | [`cursor_position_signal()`](#codeeditorhandle-cursor_position_signal) |
| `teksilo_core::Signal<usize>` | [`caret_count()`](#codeeditorhandle-caret_count) |
| `teksilo_core::Signal<Option<(usize, usize)>>` | [`bracket_match()`](#codeeditorhandle-bracket_match) |
| `teksilo_core::Signal<bool>` | [`has_selection()`](#codeeditorhandle-has_selection) |
| `teksilo_core::Signal<bool>` | [`can_undo()`](#codeeditorhandle-can_undo) |
|  | [`undo()`](#codeeditorhandle-undo) |
|  | [`redo()`](#codeeditorhandle-redo) |
|  | [`copy(ctx: &teksilo_core::widget::EventContext<'_>)`](#codeeditorhandle-copy) |
|  | [`cut(ctx: &teksilo_core::widget::EventContext<'_>)`](#codeeditorhandle-cut) |
|  | [`paste(ctx: &teksilo_core::widget::EventContext<'_>)`](#codeeditorhandle-paste) |
|  | [`select_all()`](#codeeditorhandle-select_all) |
| `bool` | [`is_read_only()`](#codeeditorhandle-is_read_only) |
| `teksilo_core::Signal<bool>` | [`can_redo()`](#codeeditorhandle-can_redo) |
| `teksilo_core::Signal<u64>` | [`document_version()`](#codeeditorhandle-document_version) |
| `teksilo_core::Signal<f32>` | [`scroll_y()`](#codeeditorhandle-scroll_y) |

### `CompletionItem`

| Returns | Function |
| ---: | :--- |
| | **Constructors** |
| `Self` | [`new(label: impl Into<String>)`](#completionitem-new) |
| | **Builder methods** |
| `Self` | [`insert_text(text: impl Into<String>)`](#completionitem-insert_text) |
| `Self` | [`detail(detail: impl Into<String>)`](#completionitem-detail) |
| `Self` | [`kind(kind: CompletionKind)`](#completionitem-kind) |

### `IndentStyle`

| Returns | Function |
| ---: | :--- |
| | **Methods** |
| `String` | [`unit()`](#indentstyle-unit) |
| `u8` | [`width()`](#indentstyle-width) |

### `BracketPair`

| Returns | Function |
| ---: | :--- |
| | **Constructors** |
| `Self` | [`new(open: char, close: char)`](#bracketpair-new) |

### `CodeConfig`

| Returns | Function |
| ---: | :--- |
| | **Methods** |
| `Option<char>` | [`closing_for(open: char)`](#codeconfig-closing_for) |
| `Option<char>` | [`opening_for(close: char)`](#codeconfig-opening_for) |

## Detailed description

Three faces over one core:

- `CodeEditor` — a source editor: gutter, current-line highlight,
  indentation, bracket handling, multiple carets.
- `PlainTextEditor` — the same core with the code affordances off and
  wrapping on: a notes field, a commit message, a description box.
- `LogView` — read-only, append-only, tail-following.

They are one implementation because they differ in *configuration*, not in
kind. All three are a monospaced-or-not run of lines with a caret in it; a
separate widget per face would triplicate the caret, selection, IME,
clipboard, scrolling, and accessibility and let them drift.

### Why not `RichTextEditor`

`RichTextEditor` already edits multi-line text, and this deliberately does
not build on it. Its command vocabulary is tables, lists, blockquotes, and
bold — reusing it would put Tab-navigates-a-table-cell and
Ctrl+B-emboldens into a source file, where the first is wrong and the second
is meaningless. Its state carries a table-aware Ctrl+A ladder and a rich
clipboard fragment; this one carries an indent policy and a caret vector.
The overlap is real but it is the *clock* — the caret blink, the debounce
window, the scroll arithmetic — and that lives in the crate-internal
`common::editor_runtime`, shared by both.

### Language-agnostic by construction

There is no `Language` enum here. Comment tokens, bracket pairs, indent
width, and highlighting are `CodeConfig` values the application supplies:
the editor knows how to toggle a line comment, not that Rust uses `//`.
Guessing would be worse than not knowing — inserting `//` into a Python file
corrupts it silently.

## Density

The picture above is the widget at `TargetDensity::Compact`, the mouse-and-keyboard ladder. Below is the same subject on the same canvas with only the ladder changed, so what moves is the density and nothing else — where the subject no longer fits, that is what the denser targets cost it at that size. See `docs/density-and-targets.md`.

**Touch**

![CodeEditorHandle at Touch density](img/code_editor-touch.png)

## API reference

📖 [Full rustdoc API for this module](https://docs.rs/teksilo-widgets/latest/teksilo_widgets/code_editor/index.html)

<a id="codeeditorhandle"></a>

## `pub struct CodeEditorHandle`

A handle onto a live editor, cloneable and detachable from the widget.

The `EditorHandle` pattern: an app keeps one to drive the editor from a
toolbar, a shortcut, or a test without holding the widget itself.

```rust
pub struct CodeEditorHandle { /* fields */ }
```

### Methods

<a id="codeeditorhandle-cursor_position"></a>

#### `pub fn cursor_position(&self) -> usize`

The caret's document position.

<a id="codeeditorhandle-cursor_position_signal"></a>

#### `pub fn cursor_position_signal(&self) -> teksilo_core::Signal<usize>`

The primary caret's document position — a character offset into the whole
document, not a line or column — as a reactive signal. Bind it in a status
bar to show a caret position that tracks every caret move, not only edits.

<a id="codeeditorhandle-caret_count"></a>

#### `pub fn caret_count(&self) -> teksilo_core::Signal<usize>`

Live caret count — `1` unless multi-caret editing is active.

<a id="codeeditorhandle-bracket_match"></a>

#### `pub fn bracket_match(&self) -> teksilo_core::Signal<Option<(usize, usize)>>`

The bracket next to the caret and its match, as document positions, or
`None`. Populated only when the editor was configured with
`match_brackets` and bracket pairs; a status surface can bind it, or an
app can read it to drive its own overlay.

<a id="codeeditorhandle-has_selection"></a>

#### `pub fn has_selection(&self) -> teksilo_core::Signal<bool>`

<a id="codeeditorhandle-can_undo"></a>

#### `pub fn can_undo(&self) -> teksilo_core::Signal<bool>`

<a id="codeeditorhandle-undo"></a>

#### `pub fn undo(&self)`

Undo this editor's last edit.

The handle could report `can_undo` long before it could
*act* on it, which left a host able to light an Undo button here and
unable to make it do anything. Ctrl+Z inside the widget always worked;
this is the same command from outside.

<a id="codeeditorhandle-redo"></a>

#### `pub fn redo(&self)`

Redo this editor's last undone edit.

<a id="codeeditorhandle-copy"></a>

#### `pub fn copy(&self, ctx: &teksilo_core::widget::EventContext<'_>)`

Copy the selection to the clipboard.

<a id="codeeditorhandle-cut"></a>

#### `pub fn cut(&self, ctx: &teksilo_core::widget::EventContext<'_>)`

Cut the selection to the clipboard.

<a id="codeeditorhandle-paste"></a>

#### `pub fn paste(&self, ctx: &teksilo_core::widget::EventContext<'_>)`

Paste over the selection.

<a id="codeeditorhandle-select_all"></a>

#### `pub fn select_all(&self)`

Select the whole document.

<a id="codeeditorhandle-is_read_only"></a>

#### `pub fn is_read_only(&self) -> bool`

Is this editor refusing edits?

<a id="codeeditorhandle-can_redo"></a>

#### `pub fn can_redo(&self) -> teksilo_core::Signal<bool>`

<a id="codeeditorhandle-document_version"></a>

#### `pub fn document_version(&self) -> teksilo_core::Signal<u64>`

Bumps on every content or format change.

<a id="codeeditorhandle-scroll_y"></a>

#### `pub fn scroll_y(&self) -> teksilo_core::Signal<f32>`

<a id="completionitem"></a>

## `pub struct CompletionItem`

A completion candidate. Build with `CompletionItem::new` and the fluent
setters; `insert_text` defaults to `label`.

```rust
pub struct CompletionItem { /* fields */ }
```

### Methods

<a id="completionitem-new"></a>

#### `pub fn new(label: impl Into<String>) -> Self`

A candidate whose inserted text is its label.

<a id="completionitem-insert_text"></a>

#### `pub fn insert_text(mut self, text: impl Into<String>) -> Self`

Override the text inserted on accept (when it differs from the label).

<a id="completionitem-detail"></a>

#### `pub fn detail(mut self, detail: impl Into<String>) -> Self`

Trailing dimmed detail (a type or signature).

<a id="completionitem-kind"></a>

#### `pub fn kind(mut self, kind: CompletionKind) -> Self`

The leading badge category.

<a id="completionkind"></a>

## `pub enum CompletionKind`

The category of a completion candidate — drives a small leading badge only.
Deliberately a fixed, language-neutral set: the editor renders a glyph, the
application decides which candidate is which kind.

```rust
pub enum CompletionKind { /* variants */ }
```

### Variants

- **`Text`**
- **`Keyword`**
- **`Function`**
- **`Method`**
- **`Variable`**
- **`Field`**
- **`Type`**
- **`Module`**
- **`Constant`**
- **`Snippet`**

<a id="completioncontext"></a>

## `pub struct CompletionContext`

What a completion provider is told about the caret when asked for candidates.

```rust
pub struct CompletionContext<'a> { /* fields */ }
```

<a id="indentstyle"></a>

## `pub enum IndentStyle`

How a line's leading indentation is written.

```rust
pub enum IndentStyle { /* variants */ }
```

### Variants

- **`Spaces`** — `width` spaces per indent level.
- **`Tabs`** — One tab character per level, rendered `width` columns wide.

### Methods

<a id="indentstyle-unit"></a>

#### `pub fn unit(&self) -> String`

The text one indent level inserts.

<a id="indentstyle-width"></a>

#### `pub fn width(&self) -> u8`

How many columns one level occupies on screen. Both styles need this:
spaces to know how many to strip on dedent, tabs to render the stop.

<a id="bracketpair"></a>

## `pub struct BracketPair`

A pair of characters the editor treats as opening and closing delimiters.

Used for auto-closing and for match highlighting. The application declares
the set, because the *same* character means different things per language:
`<` is a bracket in a generic parameter list and a less-than sign in
arithmetic, and only the caller knows which document this is.

```rust
pub struct BracketPair { /* fields */ }
```

### Methods

<a id="bracketpair-new"></a>

#### `pub const fn new(open: char, close: char) -> Self`

<a id="common_brackets"></a>

## `pub const COMMON_BRACKETS`

The three pairs that are structural in essentially every bracketed
language. A convenience starting point, not a default — an editor with no
configured pairs simply does no bracket handling, which is correct for
prose or a log.

```rust
pub const COMMON_BRACKETS: &[BracketPair] = &[
    BracketPair::new('(', ')'),
    BracketPair::new('[', ']'),
    BracketPair::new('{', '}'),
];
```

<a id="codeconfig"></a>

## `pub struct CodeConfig`

Editing behaviour the code editor applies, all supplied by the application.

```rust
pub struct CodeConfig { /* fields */ }
```

### Methods

<a id="codeconfig-closing_for"></a>

#### `pub fn closing_for(&self, open: char) -> Option<char>`

The closing partner for `open`, if it is a configured opening delimiter.

<a id="codeconfig-opening_for"></a>

#### `pub fn opening_for(&self, close: char) -> Option<char>`

The opening partner for `close`, if it is a configured closing delimiter.
