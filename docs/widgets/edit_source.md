<!-- SPDX-License-Identifier: MPL-2.0 -->
<!-- SPDX-FileCopyrightText: 2026 FernTech -->

# EditSource

`EditSource`: the input route a piece of text reached a text surface by.

## Detailed description

Shared by every surface that reports its arrivals (`RichTextEditor` and
`TextInputField` with the composites built on it), which is why it lives at
the crate root rather than in either widget's module.

## API reference

📖 [Full rustdoc API for this module](https://docs.rs/teksilo-widgets/latest/teksilo_widgets/index.html)

<a id="editsource"></a>

## `pub enum EditSource`

**Import:** `use teksilo::widgets::EditSource;`

How a piece of text reached the document: the **channel**, not the author.

Deliberately framework-generic, and deliberately small. These are the routes
a toolkit can actually observe: which input path the characters came down.
What that *means* is the application's to decide, and every application will
decide differently. A writing tool cares that dictation is not typing, a code
editor cares that a snippet is not either, and a form cares about none of it.
Teksilo says what it saw; it does not interpret.

⚠ **Not evidence of who wrote anything.** Text typed one character at a time
was typed one character at a time, and that is the entire claim. Anything
further (who, or whether a person at all) is an inference this cannot make
and no consumer of it should pretend to.

Exhaustive on purpose: an application accounting for where its text came
from wants a compile error when a new channel appears, not a wildcard arm
that quietly counts it as something else.

```rust
pub enum EditSource { /* variants */ }
```

### Variants

- **`Keyboard`** — Typed, one key at a time.
- **`Ime`** — The settled result of an IME composition (CJK/Kana candidate selection, a dead-key accent), and text the platform's input system inserts outside a key press: the emoji viewer or panel and AutoFill (they arrive as `ImeOrigin::Insertion`). Separate from `Self::Keyboard` because the characters that land are not the keys that were pressed.
- **`Dictation`** — Speech the platform's dictation recognised, through its text input system (macOS Dictation, Windows voice typing).  Separate from `Self::Accessibility`, which is an assistive technology writing through AccessKit; this is the platform's own input system.
- **`WritingTools`** — Text a system writing assistant produced to replace the user's (macOS Writing Tools).  This is machine-produced text, which an application accounting for provenance will want apart from everything else.
- **`Clipboard`** — Pasted, as plain text or as HTML.
- **`Accessibility`** — Written by an assistive technology through AccessKit's `SetValue` or `ReplaceSelectedText`: a braille display, an assistive technology's own dictation.  **Never folded into `Self::Keyboard`.** For some people this *is* typing, and a toolkit that reported it as something else, or as nothing, would be quietly erasing how they work.
- **`Programmatic`** — Inserted by the application itself rather than by anything the person at the keyboard did: a template, a substitution, a completion.
