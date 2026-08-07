<!-- SPDX-License-Identifier: MPL-2.0 -->
<!-- SPDX-FileCopyrightText: 2026 FernTech -->

# CodeEditorHandle

Multi-line plain-text and code editing surfaces.

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

# Why not `RichTextEditor`

`RichTextEditor` already edits multi-line text, and this deliberately does
not build on it. Its command vocabulary is tables, lists, blockquotes, and
bold — reusing it would put Tab-navigates-a-table-cell and
Ctrl+B-emboldens into a source file, where the first is wrong and the second
is meaningless. Its state carries a table-aware Ctrl+A ladder and a rich
clipboard fragment; this one carries an indent policy and a caret vector.
The overlap is real but it is the *clock* — the caret blink, the debounce
window, the scroll arithmetic — and that lives in the crate-internal
`common::editor_runtime`, shared by both.

# Language-agnostic by construction

There is no `Language` enum here. Comment tokens, bracket pairs, indent
width, and highlighting are `CodeConfig` values the application supplies:
the editor knows how to toggle a line comment, not that Rust uses `//`.
Guessing would be worse than not knowing — inserting `//` into a Python file
corrupts it silently.

## Builder methods at a glance

`cursor_position`, `cursor_position_signal`, `caret_count`, `bracket_match`, `has_selection`, `can_undo`, `can_redo`, `document_version`, `scroll_y`

## API reference

📖 [Full rustdoc API for this module](../api/teksilo_widgets/code_editor/index.html)

## `pub struct CodeEditorHandle`

A handle onto a live editor, cloneable and detachable from the widget.

The `EditorHandle` pattern: an app keeps one to drive the editor from a
toolbar, a shortcut, or a test without holding the widget itself.

```rust
pub struct CodeEditorHandle { /* fields */ }
```

### Methods

#### `pub fn cursor_position(&self) -> usize`

The caret's document position.

#### `pub fn cursor_position_signal(&self) -> teksilo_core::Signal<usize>`

The primary caret's document position — a character offset into the whole
document, not a line or column — as a reactive signal. Bind it in a status
bar to show a caret position that tracks every caret move, not only edits.

#### `pub fn caret_count(&self) -> teksilo_core::Signal<usize>`

Live caret count — `1` unless multi-caret editing is active.

#### `pub fn bracket_match(&self) -> teksilo_core::Signal<Option<(usize, usize)>>`

The bracket next to the caret and its match, as document positions, or
`None`. Populated only when the editor was configured with
`match_brackets` and bracket pairs; a status surface can bind it, or an
app can read it to drive its own overlay.

#### `pub fn has_selection(&self) -> teksilo_core::Signal<bool>`

#### `pub fn can_undo(&self) -> teksilo_core::Signal<bool>`

#### `pub fn can_redo(&self) -> teksilo_core::Signal<bool>`

#### `pub fn document_version(&self) -> teksilo_core::Signal<u64>`

Bumps on every content or format change.

#### `pub fn scroll_y(&self) -> teksilo_core::Signal<f32>`
