// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Rich text editor demo with a live preview pane.
//!
//! Milestone 8b of §27.10. Shows the editable preset
//! `RichTextEditor::editor` driving a shared `TextDocument` that is
//! ALSO bound to a read-only `RichTextEditor::read_only` in the
//! second pane. Both editors subscribe to `doc.on_change()`
//! independently, so edits in the left pane propagate to the right
//! pane on the next frame tick. An input method's composition shows only
//! in the left pane, the view it is made in: the preview shows the text as
//! recorded until the composition is committed or cancelled.
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
//!   * The Edit menu (Undo / Redo / Cut / Copy / Paste / Paste without
//!     formatting / Select All) is a native menu bar on macOS, an in-window
//!     one elsewhere. On macOS the system adds its own rows to it: Start
//!     Dictation, Emoji & Symbols, AutoFill and Writing Tools. Try them with
//!     the focus in the editor, then in the Title field.
//!   * The Title field above the panes is a plain `TextInput`, to compare
//!     with the editor when the system inserts text. Its own "Title: typed
//!     ..." line counts what reached it, the same way.
//!   * The "Writing Tools (macOS)" checkbox next to the Title turns the
//!     system's Writing Tools off and on for both the Title field and the
//!     editor (`.writing_tools(..)`). With it off, the Writing Tools row of
//!     the Edit menu and the context menu should grey out in both.
//!   * Links are followed through `.on_link_activated(...)`, installed on
//!     both panes: Ctrl+click a link in the editor, or use its context
//!     menu's Open Link; in the preview click it, or Tab to it (the
//!     preview's links are Tab stops while it follows them) and press Enter.
//!     The last line under the panes names the link followed instead of
//!     opening it.
//!   * The "Editor: typed ..." line under the panes counts the characters
//!     that reached the editable editor, per source (`on_text_inserted`):
//!     typed, IME, dictated, Writing Tools, pasted, assistive technology,
//!     programmatic.
//!   * Right-click opens the built-in Cut / Copy / Paste / Paste
//!     Unformatted / Select All menu. Items reflect the live
//!     selection / policy state — Cut and Copy are greyed when there
//!     is no selection; the read-only preset shows only Copy +
//!     Select All.
//!   * Double-click a word → selects it. Triple-click → selects the
//!     paragraph. Both via cooperative double/triple tap recognizers.
//!   * Drag from inside text to near the top or bottom edge → selection
//!     extends, viewport auto-scrolls. **Mouse only**: a direct pointer's drag
//!     belongs to the editor's own pan claim, so a finger scrolls instead. A
//!     finger selects by holding a word and then dragging a handle, and a handle
//!     dragged to the edge does scroll the view to follow it.
//!   * Use the formatting toolbar between the dark-mode bar and the
//!     editor: Bold / Italic / Underline / Strikethrough tint when the
//!     caret sits in that format; alignment buttons reflect the
//!     current block; the heading picker (Normal / H1–H6) tracks the
//!     caret's block and applies on pick; Bullet / Numbered / Indent /
//!     Outdent operate on lists; Insert Table drops a 3×3 table at
//!     the caret; Undo / Redo enable from `can_undo` / `can_redo`.
//!   * The second toolbar row is dedicated to table operations and
//!     enables only when the caret is inside a table.
//!   * The third toolbar row showcases the document-level
//!     `SyntaxHighlighter`. Pick Off / Search / Syntax / Spell:
//!       - **Search** highlights every match of the live query (yellow
//!         background) — type in the field and watch the *editor* pane.
//!       - **Syntax** colors Rust-ish keywords, numbers and string
//!         literals across the document.
//!       - **Spell** underlines a seeded set of misspelled words with a
//!         red wavy spell-check underline.
//!
//! The highlighter lives on the `TextDocument`, but whether to *show*
//! it is a per-view choice: the editable pane displays the highlights
//! while the read-only preview stays **bare** — search / spell / syntax
//! are authoring affordances, not part of the rendered document. The
//! `read_only` preset defaults to `show_highlights(false)`; a read-only
//! *syntax* viewer would opt back in with `.show_highlights(true)`. The
//! bare view also pulls a clean snapshot and ignores paint-only highlight
//! events, so it does zero work as you type in the search field. Either
//! way the shadow formatting never touches stored content (invisible to
//! undo / copy / export). See `highlighters.rs` and `highlight_controls.rs`.
//!
//! Run with: `cargo run -p rich-text-editor`

mod format_toolbar;
mod highlight_controls;
mod highlighters;

use teksilo::prelude::*;
use teksilo::text_document::{Alignment, BlockFormat, MoveMode, TextDocument};
use teksilo::widgets::EditSource;
use teksilo::widgets::rich_text::{RichTextEditor, ScrollPolicy};
use teksilo::widgets::{
    Checkbox, Expand, HStack, MenuBar, MenuEntry, MenuModel, NativeMenuMode, Orientation,
    PaneDescriptor, Spacer, Splitter, SplitterModel, StandardMenu, TextInput, TextWidget, Toolbar,
};

use format_toolbar::FormatToolbar;
use highlight_controls::HighlightControls;

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

/// The minimum menu bar: the App menu and an Edit menu. AppKit adds its own
/// rows (Start Dictation, Emoji & Symbols, AutoFill, Writing Tools) to the menu
/// titled "Edit".
fn menu_model() -> MenuModel {
    MenuModel::new()
        .standard_menu(StandardMenu::app())
        .menu(lit!("&Edit"), |m| {
            m.item(
                MenuEntry::new(lit!("&Undo"))
                    .intent("edit.undo")
                    .shortcut("edit.undo"),
            )
            .item(
                MenuEntry::new(lit!("&Redo"))
                    .intent("edit.redo")
                    .shortcut("edit.redo"),
            )
            .separator()
            .item(
                MenuEntry::new(lit!("Cu&t"))
                    .intent("edit.cut")
                    .shortcut("edit.cut"),
            )
            .item(
                MenuEntry::new(lit!("&Copy"))
                    .intent("edit.copy")
                    .shortcut("edit.copy"),
            )
            .item(
                MenuEntry::new(lit!("&Paste"))
                    .intent("edit.paste")
                    .shortcut("edit.paste"),
            )
            .item(
                MenuEntry::new(lit!("Paste without &formatting"))
                    .intent("edit.paste_plain")
                    .shortcut("edit.paste_plain"),
            )
            .item(
                MenuEntry::new(lit!("Select &All"))
                    .intent("edit.select_all")
                    .shortcut("edit.select_all"),
            )
        })
}

/// Characters that reached one field, per source.
#[derive(Default, Clone, Copy)]
struct Arrivals {
    typed: usize,
    ime: usize,
    dictated: usize,
    writing_tools: usize,
    pasted: usize,
    assistive: usize,
    programmatic: usize,
}

impl Arrivals {
    /// Exhaustive on purpose: a new `EditSource` must break this build.
    fn record(&mut self, source: EditSource, chars: usize) {
        match source {
            EditSource::Keyboard => self.typed += chars,
            EditSource::Ime => self.ime += chars,
            EditSource::Dictation => self.dictated += chars,
            EditSource::WritingTools => self.writing_tools += chars,
            EditSource::Clipboard => self.pasted += chars,
            EditSource::Accessibility => self.assistive += chars,
            EditSource::Programmatic => self.programmatic += chars,
        }
    }

    fn line(&self, field: &str) -> String {
        format!(
            "{field}: typed {}, IME {}, dictated {}, writing tools {}, pasted {}, assistive {}, \
             programmatic {}",
            self.typed,
            self.ime,
            self.dictated,
            self.writing_tools,
            self.pasted,
            self.assistive,
            self.programmatic
        )
    }
}

fn dark_mode_toolbar() -> impl Widget {
    Toolbar::new().child(
        HStack::new()
            .child(Spacer::new())
            .child(teksilo::widgets::ThemeSwitcher::new()),
    )
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

The **Spell** highlighter underlines deliberately misspelled words so the
spell-check squiggle is visible: you will definately recieve teh seperate
squiggles wich occured here — alot of them, untill you pick another mode.

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
  extending the selection. With a finger a drag scrolls instead: hold a
  word to select it, then drag one of the two handles.
- Tab at the start of a list item increases the indent; Shift+Tab
  decreases it. Backspace at the start of an indented list item
  dedents; at indent 0 it exits the list.
- Right-click — or hold, with a finger — for Cut / Copy / Paste / Paste
  Unformatted / Select All; items grey out when not applicable (Cut/Copy
  without a selection, Select All in an empty document). A hold also
  raises the touch selection toolbar.
- Scroll with the mouse wheel, or drag with a finger; overlay scrollbars
  fade in on the right/bottom edges.

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
    // The two passes above are recorded edits. Left in the history, an undo
    // past the writer's first edit takes the sample's margins and alignments
    // away one block at a time, which reads as undo doing nothing.
    doc.clear_undo_redo();

    TeksiloAppBuilder::new()
        .install_automation_bridge_in_debug()
        .install_inspector_in_debug()
        .theme(teksilo::presets::intui::light())
        .initial_window(
            WindowConfig::new()
                .title("Teksilo — Rich Text Editor")
                .size(1100, 640)
                .root(move |tree, _state| {
                    // Build the editable view first so the formatting
                    // toolbar can borrow it for signal / handle wiring,
                    // then move the editor into the SplitView.
                    let editor_arrivals = Signal::new(Arrivals::default());
                    let editor_line = editor_arrivals.map(|a| a.line("Editor"));
                    let writing_tools = Signal::new(true);
                    // A followed link (in either pane) is shown on the status
                    // line under the panes rather than opened.
                    let link_line = Signal::new(String::from(
                        "Links: Ctrl+click one in the editor (Cmd-click on macOS); in the preview, click one, or Tab to it and press Enter.",
                    ));
                    let editor_link_line = link_line.clone();
                    let preview_link_line = link_line.clone();
                    let editor = RichTextEditor::editor(doc.clone())
                        .content_padding_symmetric(0.0, 10.0)
                        .writing_tools(writing_tools.clone())
                        .label(lit!("Editor"))
                        .on_link_activated(move |href, _ctx| {
                            editor_link_line.set(format!("Followed link from the editor: {href}"));
                        })
                        .on_text_inserted(move |source, chars| {
                            let mut a = editor_arrivals.get();
                            a.record(source, chars);
                            editor_arrivals.set(a);
                        });
                    let title = Signal::new(String::new());
                    let title_arrivals = Signal::new(Arrivals::default());
                    let title_line = title_arrivals.map(|a| a.line("Title"));
                    let toolbar = FormatToolbar::new(&editor);
                    let highlight_controls = HighlightControls::new(&doc);
                    let doc_preview = doc.clone();
                    // Editor | preview, ~55 / 45 split that grows
                    // proportionally on resize (both panes stretch).
                    let layout = SplitterModel::from_panes(
                        vec![
                            PaneDescriptor::new().stretch(55.0),
                            PaneDescriptor::new().stretch(45.0),
                        ],
                        Orientation::Horizontal,
                    );
                    tree.add(
                        teksilo::widgets::VStack::new()
                            .child(
                                MenuBar::from_model(menu_model())
                                    .native_on_macos(NativeMenuMode::Suppress),
                            )
                            .child(dark_mode_toolbar())
                            .child(
                                HStack::new()
                                    .spacing(8.0)
                                    // The field is named by `label`; the visible
                                    // caption is hidden from AT so it is not read twice.
                                    .child(TextWidget::new(lit!("Title")).a11y_hidden())
                                    // `horizontal()`: the default `Expand::new()` also
                                    // claims the vertical axis at a zero basis, so it
                                    // reports a height of 0 to the row, which then sizes
                                    // to the caption and lets the taller field spill
                                    // over the toolbar below.
                                    .child(
                                        Expand::horizontal().child(
                                            TextInput::new(title)
                                                .label(lit!("Title"))
                                                .writing_tools(writing_tools.clone())
                                                .on_text_inserted(move |source, chars| {
                                                    let mut a = title_arrivals.get();
                                                    a.record(source, chars);
                                                    title_arrivals.set(a);
                                                }),
                                        ),
                                    )
                                    .child(
                                        Checkbox::new(writing_tools)
                                            .label(lit!("Writing Tools (macOS)")),
                                    ),
                            )
                            .child(toolbar)
                            .child(highlight_controls)
                            .child(
                                Expand::new().child(
                                    Splitter::new(layout).pane(editor).pane(
                                        // The preview stays bare of highlights:
                                        // `read_only` defaults to
                                        // `show_highlights(false)`. A read-only
                                        // *syntax* viewer would opt back in with
                                        // `.show_highlights(true)`. A
                                        // composition shows in the editor it
                                        // is made in only.
                                        RichTextEditor::read_only(doc_preview)
                                            .v_scroll_policy(ScrollPolicy::Auto)
                                            .label(lit!("Preview"))
                                            .on_link_activated(move |href, _ctx| {
                                                preview_link_line.set(format!(
                                                    "Followed link from the preview: {href}"
                                                ));
                                            }),
                                    ),
                                ),
                            )
                            // One line per field.
                            .child(TextWidget::new(lit!("")).text(title_line))
                            .child(TextWidget::new(lit!("")).text(editor_line))
                            .child(TextWidget::new(lit!("")).text(link_line)),
                    )
                }),
        )
        .install_native_menu()
        .run();
}
