//! Rich text editor demo with a live preview pane.
//!
//! Milestone 8b of §27.10. Shows the editable preset
//! `RichTextEditor::editor` driving a shared `TextDocument` that is
//! ALSO bound to a read-only `RichTextEditor::read_only` in the
//! second pane. Both editors subscribe to `doc.on_change()`
//! independently, so edits in the left pane propagate to the right
//! pane on the next frame tick.
//!
//! Try it:
//!   * Type to insert text; bursts are batched through `pending_chars`
//!     so a typing storm fires one `text_changed` pulse per 150 ms.
//!   * Backspace / Delete / Ctrl+Backspace / Ctrl+Delete for character
//!     and word deletion.
//!   * Enter to split blocks.
//!   * Ctrl+B / Ctrl+I / Ctrl+U to toggle formatting on a selection.
//!   * Ctrl+Z / Ctrl+Y for undo / redo.
//!   * Ctrl+A once to select the block, again to escalate (inside a
//!     table: cell → table → document; outside: single-shot document).
//!   * Ctrl+C / Ctrl+X / Ctrl+V / Ctrl+Shift+V for copy / cut / paste /
//!     paste as plain text. The editor writes both HTML and plain
//!     payloads on copy so rich paste into Firefox, Word, or Google
//!     Docs keeps the formatting; paste from those apps parses their
//!     HTML back into the document.
//!   * Right-click opens the built-in Cut / Copy / Paste / Paste
//!     Unformatted / Select All menu. Items reflect the live
//!     selection / policy state — Cut and Copy are greyed when there
//!     is no selection; the read-only preset shows only Copy +
//!     Select All.
//!   * Double-click a word → selects it. Triple-click → selects the
//!     paragraph. Both via cooperative double/triple tap recognizers.
//!   * Drag from inside text to near the top or bottom edge → selection
//!     extends, viewport auto-scrolls.
//!
//! Run with: `cargo run -p rich-text-editor --features "rich-text clipboard"`

use bastyde::prelude::*;
use bastyde::text_document::{Alignment, BlockFormat, MoveMode, TextDocument};
use bastyde::widgets::rich_text::{RichTextEditor, ScrollPolicy};
use bastyde::widgets::{Button, Expand, HStack, Spacer, SplitView, Toolbar};

/// Walk every block in `doc` and apply default vertical spacing per
/// kind: roomier margins before / after headings (scaled by heading
/// level), tighter margins between body paragraphs, a touch of breathing
/// room above and below fenced code blocks. The markdown importer
/// doesn't emit per-block margins itself, so without this pass every
/// block is glued to its neighbour — readable, but visually busy.
///
/// Implemented as a per-block cursor + `set_block_format` so the call
/// goes through the normal undo / event pipeline. Run BEFORE the
/// document is bound to any RichTextEditor so the resulting layout is
/// the first thing the user sees.
fn apply_default_margins(doc: &TextDocument) {
    for block in doc.blocks() {
        let heading_level = block.block_format().heading_level;
        let (top, bottom) = match heading_level {
            Some(1) => (24, 12),
            Some(2) => (20, 10),
            Some(3) => (16, 8),
            Some(4) => (12, 6),
            Some(_) => (10, 4), // H5, H6
            None => (4, 4),     // body paragraph / list item / code block
        };
        let cursor = doc.cursor_at(block.position());
        cursor.set_position(block.position(), MoveMode::MoveAnchor);
        let _ = cursor.set_block_format(&BlockFormat {
            top_margin: Some(top),
            bottom_margin: Some(bottom),
            ..BlockFormat::default()
        });
    }
}

/// Demo the four `Alignment` variants and `text_indent` (first-line
/// indent) — neither is reachable from markdown syntax, so they're
/// applied programmatically after the import. Matches blocks by a
/// sentinel prefix in the block's first line; cheap and obvious, fine
/// for a fixed showcase doc.
fn apply_alignment_demos(doc: &TextDocument) {
    for block in doc.blocks() {
        let text = block.text();
        let trimmed = text.trim_start();
        let (alignment, text_indent) = if trimmed.starts_with("[Center]") {
            (Some(Alignment::Center), None)
        } else if trimmed.starts_with("[Right]") {
            (Some(Alignment::Right), None)
        } else if trimmed.starts_with("[Justify]") {
            (Some(Alignment::Justify), None)
        } else if trimmed.starts_with("[Indent]") {
            // text_indent shifts the FIRST line right by N pixels;
            // subsequent wrapped lines stay flush with the block's
            // left margin. Classic book / academic-style paragraph.
            (None, Some(32))
        } else {
            continue;
        };
        let prior = block.block_format();
        let cursor = doc.cursor_at(block.position());
        cursor.set_position(block.position(), MoveMode::MoveAnchor);
        let _ = cursor.set_block_format(&BlockFormat {
            alignment,
            text_indent,
            // Preserve the per-kind margins applied earlier.
            top_margin: prior.top_margin,
            bottom_margin: prior.bottom_margin,
            ..BlockFormat::default()
        });
    }
}

fn dark_mode_toolbar() -> impl Widget {
    let is_dark = Signal::new(false);
    Toolbar::new().child(HStack::new().child(Spacer::new()).child(
        Button::new_literal("Toggle Dark Mode").on_activate_fn(move |ctx| {
            let next = !is_dark.get();
            is_dark.set(next);
            ctx.set_theme(if next {
                bastyde::presets::intui::dark()
            } else {
                bastyde::presets::intui::light()
            });
        }),
    ))
}

const SAMPLE: &str = r#"# RichTextEditor — Capability Showcase

This window hosts **two** `RichTextEditor` widgets bound to the *same*
`TextDocument`. The left pane is the full editor; the right pane is a
read-only viewer. Edits on the left propagate live to the right on the
next frame tick.

This document exercises every feature the markdown importer recognises.
Capabilities listed under "API-only features" exist in the engine but
have no markdown syntax — they are reachable through keyboard
shortcuts (Ctrl+U, Ctrl+B, …) or `TextDocument`'s typed API.

## Heading scale

The typesetter scales heading sizes against the body font:
H1 = 2.0×, H2 = 1.5×, H3 = 1.25×, H4 = 1.1×, H5/H6 = 1.0×.

### Third-level heading

#### Fourth-level heading

##### Fifth-level heading

###### Sixth-level heading

## Inline character formatting

Markdown can express **bold**, *italic*, ***bold italic***, and
~~strikethrough~~ inline. Inline `code` switches to the monospace
family. Links like [the text-document repo](https://example.com/text-document)
carry an `anchor_href` and are clickable via the editor's
`.on_link_activated(...)` callback.

You can mix runs freely: a paragraph with **bold *and italic together***,
a [**bold link**](https://example.com), `code with ~~strike inside~~`,
and a final ***`bold-italic code`*** run.

### API-only character features (try the keyboard)

- **Underline** — Ctrl+U. The engine supports five underline styles
  (`Single`, `Dash`, `Dot`, `DashDot`, `DashDotDot`) plus a `Wave`
  variant used by spell-checkers.
- **Overline** — API only.
- **Superscript / subscript** — `vertical_alignment` field on
  `CharacterFormat`; rendered at 65% of the base font size.
- **Foreground / background colour** — per-character.
- **Letter / word spacing**, **font family / size / weight** override,
  **tooltips**, **named anchors** — all API only.

## Lists

### Unordered (Disc) with nesting

- First item at indent 0
- Second item, mixing **bold** and *italic* in the same line
  - Nested item at indent 1
  - Another nested item with `inline code`
    - Deeper nesting at indent 2
    - One more at indent 2
- Back to indent 0 with a [link](https://example.com)

### Ordered (Decimal) with nesting

1. First numbered item
2. Second numbered item
   1. Nested decimal at indent 1
   2. Another nested decimal
      1. Triple-nested decimal at indent 2
3. Back to indent 0

### API-only list styles

`ListStyle` has eight variants total. The markdown importer only
emits `Disc` and `Decimal`. The remaining six — `Circle`, `Square`,
`LowerAlpha`, `UpperAlpha`, `LowerRoman`, `UpperRoman` — plus
custom `prefix` / `suffix` strings, are reachable through
`TextCursor::create_list(...)`.

## Blockquotes

> A single-level blockquote. Nestable to arbitrary depth and renders
> with an indented left margin.
>
> > Nested blockquote at depth 2. Inline formatting works inside:
> > **bold**, *italic*, ~~strike~~, `code`, [links](https://example.com).
> >
> > > A third-level nested blockquote, for good measure.

## Code blocks

Fenced code blocks remember the language tag for syntax-highlighter
hooks. The block paints with a light-grey background, switches to the
monospace family, and disables word-wrap.

```rust
fn fibonacci(n: u64) -> u64 {
    match n {
        0 | 1 => n,
        _ => fibonacci(n - 1) + fibonacci(n - 2),
    }
}
```

```python
def fib(n):
    return n if n < 2 else fib(n - 1) + fib(n - 2)
```

```
A fenced block with no language tag — same monospace + grey
background treatment, but `code_language` stays `None`.
```

## Tables (GitHub-flavored)

The markdown importer recognises GFM table syntax. Header and body
cells go through the full inline-formatting pipeline, so any of the
inline features above work per cell.

| Feature           | Markdown      | API only |
|-------------------|---------------|----------|
| **Bold**          | `**text**`    | —        |
| *Italic*          | `*text*`      | —        |
| ~~Strikethrough~~ | `~~text~~`    | —        |
| `inline code`     | `` `text` ``  | —        |
| [Link](https://x) | `[t](url)`    | —        |
| Underline         | —             | Ctrl+U   |
| Super / subscript | —             | API      |
| Text colour       | —             | API      |
| Image inline      | —             | API      |

Tables are fully editable — Tab moves to the next cell (auto-inserts
a row at the last cell), Shift+Tab is the inverse, and Shift+Arrow
at a cell boundary activates rectangular cell selection.

### API-only table features

- `column_span` / `row_span` on `TableCell` (markdown emits 1×1 only).
- Per-cell `padding`, `border`, `vertical_alignment`,
  `background_color`.
- Table-level `border`, `cell_spacing`, `cell_padding`, `width`,
  `alignment`.
- Explicit `column_widths`.
- GFM column alignment (`|:---:|`) — parsed but currently discarded.

## Alignment & first-line indent (API only)

`BlockFormat::alignment` (Left / Right / Center / Justify) and
`BlockFormat::text_indent` (first-line indent in pixels) have no
markdown syntax — the four paragraphs below are reformatted at
startup by `apply_alignment_demos(&doc)` based on a sentinel prefix
on each one.

[Center] Centered paragraph. Each line in this block is shifted right by half the slack so the text is centred against the block's content width. Useful for posters and dialog titles.

[Right] Right-aligned paragraph — each line is shifted to butt against the right margin of the block. Common in numbers / RTL UIs.

[Justify] Justified paragraph that needs enough text to wrap onto more than one line so the inter-word spacing stretches visibly. Words on every line except the last get pushed apart to make their line span the full content width. The last line is left-aligned per typographic convention.

[Indent] Paragraph with a 32-px first-line indent. The first line starts after a horizontal gap; subsequent wrapped lines start flush with the block's left margin. This is the classic book / academic style for unspaced paragraphs — pad the text out so wrapping is visible: the indent only applies to the first wrapped line, every other line lands flush with the block's left edge.

## Markdown features NOT imported

The pulldown-cmark parser sees these tokens, but the importer's event
loop drops them on the floor:

- Horizontal rules (`---`, `***`, `___`) — silently ignored.
- `![alt](url)` inline images — alt text may appear as literal text;
  the image is not inserted. Use `TextCursor::insert_image(...)` to
  embed an image via the API.
- Task-list checkboxes (`- [ ]`, `- [x]`) — the `[ ]` text appears
  inline; the block's `marker` field stays `NoMarker`.
- Inline HTML blocks (`<div>`, `<img>`, …) — dropped.
- Footnotes — dropped.

## What to try

- Type to insert text; bursts batch through `pending_chars` so a
  typing storm fires one `text_changed` pulse per 150 ms.
- Backspace / Delete / Ctrl+Backspace / Ctrl+Delete for character
  and word deletion.
- Enter splits a block; Ctrl+Enter always inserts a block even
  inside a table cell.
- Ctrl+B / Ctrl+I / Ctrl+U toggle bold / italic / underline on the
  selection.
- Ctrl+Z / Ctrl+Y / Ctrl+Shift+Z for undo / redo. Snapshot-based
  undo means even table-shape and list-indent changes round-trip
  precisely.
- Ctrl+A once for select-all (4-level escalation ladder activates
  only inside table cells).
- Ctrl+C / Ctrl+X / Ctrl+V / Ctrl+Shift+V for copy / cut / paste /
  paste-as-plain-text. Cross-application paste round-trips through
  HTML (Linux `text/html`, macOS `public.html`, Windows `CF_HTML`),
  so copying from Firefox / Word / Google Docs preserves headings,
  bold, italic, lists, tables.
- Double-click selects a word, triple-click selects the paragraph.
- Drag past the top or bottom edge to engage auto-scroll while
  extending the selection.
- Tab at the start of a list item increases the indent; Shift+Tab
  decreases it. Backspace at the start of an indented list item
  dedents; at indent 0 it exits the list.
- Right-click for Cut / Copy / Paste / Paste Unformatted / Select
  All — items grey out when not applicable (Cut/Copy without a
  selection, Select All in an empty document).
- Scroll with the mouse wheel — overlay scrollbars fade in on the
  right/bottom edges.

Type anywhere below; watch the preview pane mirror every edit.
"#;

fn main() {
    let doc = TextDocument::new();
    doc.set_markdown(SAMPLE)
        .expect("embedded markdown should parse")
        .wait()
        .expect("markdown import completes");
    apply_default_margins(&doc);
    apply_alignment_demos(&doc);

    BastydeAppBuilder::new()
        .install_inspector_in_debug()
        .theme(bastyde::presets::intui::light())
        .initial_window(
            WindowConfig::new()
                .title("Bastyde — Rich Text Editor")
                .size(1100, 640)
                .root(move |tree, _state| {
                    let doc_editor = doc.clone();
                    let doc_preview = doc.clone();
                    let split = Signal::new(0.55);
                    tree.add(
                        bastyde::widgets::VStack::new()
                            .child(dark_mode_toolbar())
                            .child(
                                Expand::new().child(
                                    SplitView::new(split)
                                        .first(RichTextEditor::editor(doc_editor))
                                        .second(
                                            RichTextEditor::read_only(doc_preview)
                                                .v_scroll_policy(ScrollPolicy::Auto),
                                        ),
                                ),
                            ),
                    )
                }),
        )
        .run();
}
