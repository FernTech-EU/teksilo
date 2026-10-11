<!-- SPDX-License-Identifier: MPL-2.0 -->
<!-- SPDX-FileCopyrightText: 2026 FernTech -->

# FormatShortcut

The editors' own rebindable commands: bold and the rest of
`RichTextEditor`'s formatting
(`FormatShortcut`), comment and duplicate in
`CodeEditor` (`CodeShortcut`).

## Public types

| Kind | Name |
| ---: | :--- |
| `const` | [`FORMAT_CATEGORY`](#format_category) — The settings-UI category of every `FormatShortcut` |
| `const` | [`CODE_CATEGORY`](#code_category) — The settings-UI category of every `CodeShortcut` |
| `enum` | [`FormatShortcut`](#formatshortcut) — A formatting command of `RichTextEditor` |
| `enum` | [`CodeShortcut`](#codeshortcut) — A command of `CodeEditor` |

## Public functions

### `FormatShortcut`

| Returns | Function |
| ---: | :--- |
| | **Constructors** |
| `Option<Self>` | [`from_id(id: &str)`](#formatshortcut-from_id) |
| | **Methods** |
| `&'static str` | [`id()`](#formatshortcut-id) |
| `Shortcut` | [`shortcut()`](#formatshortcut-shortcut) |
| | **Constants and types** |
| `[FormatShortcut` | [`ALL`](#formatshortcut-all) |

### `CodeShortcut`

| Returns | Function |
| ---: | :--- |
| | **Constructors** |
| `Option<Self>` | [`from_id(id: &str)`](#codeshortcut-from_id) |
| | **Methods** |
| `&'static str` | [`id()`](#codeshortcut-id) |
| `Shortcut` | [`shortcut()`](#codeshortcut-shortcut) |
| | **Constants and types** |
| `[CodeShortcut` | [`ALL`](#codeshortcut-all) |

## Detailed description

They work as the standard edit commands do
(`StandardEdit`): a global shortcut
whose id is also the intent name, and an action on each editor that runs
it, so the focused editor answers. Where the standard set is registered in
every tree, these exist only while an editor that runs them does: each
editable instance declares them with
`register_shortcut_shared`,
and the shortcut goes with the last. A read-only viewer declares none. A
settings UI therefore lists Bold only in an application that has a
rich-text editor, and a user's binding waits in the registry for the next
one.

An application that binds one of these chords to a global shortcut of its
own shares it with the editor's while an editor exists.

## API reference

📖 [Full rustdoc API for this module](https://docs.rs/teksilo-widgets/latest/teksilo_widgets/editor_shortcuts/index.html)

<a id="format_category"></a>

## `pub const FORMAT_CATEGORY`

**Import:** `use teksilo::widgets::editor_shortcuts::FORMAT_CATEGORY;`

The settings-UI category of every `FormatShortcut`.

```rust
pub const FORMAT_CATEGORY: &str = "format";
```

<a id="code_category"></a>

## `pub const CODE_CATEGORY`

**Import:** `use teksilo::widgets::editor_shortcuts::CODE_CATEGORY;`

The settings-UI category of every `CodeShortcut`.

```rust
pub const CODE_CATEGORY: &str = "code";
```

<a id="formatshortcut"></a>

## `pub enum FormatShortcut`

**Import:** `use teksilo::widgets::FormatShortcut;`

A formatting command of `RichTextEditor`. Its `id` is both the
shortcut id and the intent name.

```rust
pub enum FormatShortcut { /* variants */ }
```

### Variants

- **`Bold`** — `format.bold`, Ctrl+B (⌘B on macOS).
- **`Italic`** — `format.italic`, Ctrl+I (⌘I on macOS).
- **`Underline`** — `format.underline`, Ctrl+U (⌘U on macOS).
- **`InsertParagraph`** — `format.insert_paragraph`, Ctrl+Enter (⌘Enter on macOS): a new paragraph even inside a table cell, where Enter moves to the cell below.

### Methods

<a id="formatshortcut-all"></a>

#### `pub const ALL: `FormatShortcut;`

Every formatting command, in the order a settings UI lists them.

<a id="formatshortcut-id"></a>

#### `pub const fn id(self) -> &'static str`

The shortcut id, which is also the intent name.

<a id="formatshortcut-from_id"></a>

#### `pub fn from_id(id: &str) -> Option<Self>`

The command whose [`id`` is `id`.

<a id="formatshortcut-shortcut"></a>

#### `pub fn shortcut(self) -> Shortcut`

The shortcut an editor declares for this command: its default chord, a
name in the active language, and the `FORMAT_CATEGORY` category.

<a id="codeshortcut"></a>

## `pub enum CodeShortcut`

**Import:** `use teksilo::widgets::CodeShortcut;`

A command of `CodeEditor`. Its `id` is both the shortcut id and
the intent name.

```rust
pub enum CodeShortcut { /* variants */ }
```

### Variants

- **`ToggleComment`** — `code.toggle_comment`, Ctrl+/ (⌘/ on macOS), and Ctrl+Shift+/ for the layouts that type `/` with Shift (AZERTY, German).
- **`Duplicate`** — `code.duplicate`, Ctrl+D (⌘D on macOS): the caret's line, or the selection, below itself.

### Methods

<a id="codeshortcut-all"></a>

#### `pub const ALL: `CodeShortcut;`

Every code command, in the order a settings UI lists them.

<a id="codeshortcut-id"></a>

#### `pub const fn id(self) -> &'static str`

The shortcut id, which is also the intent name.

<a id="codeshortcut-from_id"></a>

#### `pub fn from_id(id: &str) -> Option<Self>`

The command whose [`id`` is `id`.

<a id="codeshortcut-shortcut"></a>

#### `pub fn shortcut(self) -> Shortcut`

The shortcut an editor declares for this command: its default chords, a
name in the active language, and the `CODE_CATEGORY` category.
