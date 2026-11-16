<!-- SPDX-License-Identifier: MPL-2.0 -->
<!-- SPDX-FileCopyrightText: 2026 FernTech -->

# TextBaseDirection

The base direction of a text a user types: the direction its paragraph
reads in, which decides where the text starts, where an empty field puts
its caret, and which way a screen reader is told it runs.

## Public functions

### `TextBaseDirection`

| Returns | Function |
| ---: | :--- |
| | **Methods** |
| `LayoutDirection` | [`resolve(text: &str, language: Option<&LanguageTag>, window: LayoutDirection)`](#textbasedirection-resolve) |

## Detailed description

A text input resolves it, every time it lays out, in this order:

1. an explicit `TextBaseDirection::LeftToRight` or
   `TextBaseDirection::RightToLeft`, used as given;
2. with `TextBaseDirection::Auto` (the default), the first strongly
   directional character of the text (UAX #9, rules P2 and P3): Arabic
   text reads right to left, Latin text left to right, whatever the
   window's direction;
3. when the text has none (it is empty, or only digits and punctuation),
   the `content_language`
   declared on or above the field, through
   `LanguageTag::likely_direction`;
4. with no language declared, the window's layout direction.

So an empty field declared Arabic opens with its caret on the right, and a
field declared Japanese that holds Arabic text still reads right to left:
the language never decides against the text. This is HTML's `dir="auto"`,
falling back on the language before the surrounding direction.

## API reference

📖 [Full rustdoc API for this module](https://docs.rs/teksilo-widgets/latest/teksilo_widgets/text_base_direction/index.html)

<a id="textbasedirection"></a>

## `pub enum TextBaseDirection`

**Import:** `use teksilo::widgets::TextBaseDirection;`

Which way a typed text's paragraph reads. See the `module` for how
a text input resolves it.

```rust
pub enum TextBaseDirection { /* variants */ }
```

### Variants

- **`Auto`** — The text decides: its first strongly directional character, else the declared language's usual direction, else the window's.
- **`LeftToRight`** — Left to right, whatever the text.
- **`RightToLeft`** — Right to left, whatever the text.

### Methods

<a id="textbasedirection-resolve"></a>

#### `pub fn resolve( self, text: &str, language: Option<&LanguageTag>, window: LayoutDirection, ) -> LayoutDirection`

The direction a paragraph of `text` reads in: see the `module`.
`language` is the content language declared on or above the field,
`window` the window's layout direction.
