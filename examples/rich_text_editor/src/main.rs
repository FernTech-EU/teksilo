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

use fern_ui::prelude::*;
use fern_ui::text_document::TextDocument;
use fern_ui::widgets::SplitView;
use fern_ui::widgets::rich_text::{RichTextEditor, ScrollPolicy};

const SAMPLE: &str = r#"# Rich Text Editor — Preview Pane

This window hosts two `RichTextEditor` widgets bound to the **same**
`TextDocument`. The left pane is the full editor; the right pane is a
read-only viewer with a `SelectionType::Document` fallback. Because
both subscribe to `doc.on_change()` independently, edits in the left
pane propagate live to the right pane on the next frame tick — no
manual state shuffling, no `poll_events()` starvation problem.

## What works in M8b

- Full text insertion: typing, Enter to split blocks, Backspace,
  Delete, Ctrl+Backspace / Ctrl+Delete for word-level deletion.
- Undo / redo (Ctrl+Z / Ctrl+Y / Ctrl+Shift+Z).
- Bold / italic / underline toggles (Ctrl+B / Ctrl+I / Ctrl+U).
- Click to place caret (click 1), double-click to select word
  (click 2), triple-click to select paragraph (click 3). The three
  gestures are independent cooperative recognizers — click 3 escalates
  over what click 2 installed.
- Drag-select with near-edge auto-scroll. Pull the mouse past the
  top or bottom of the viewport; the widget keeps scrolling while the
  button is held.
- Copy / cut / paste through the system clipboard. In-process paste
  preserves rich formatting via a stored `DocumentFragment`;
  inter-application paste round-trips through HTML on Linux
  (`text/html`), macOS (`public.html`), and Windows (`CF_HTML`), so
  copy from Firefox / Word / Google Docs keeps headings, bold,
  italic, lists, tables — anything text-document's HTML importer
  recognises.
- Ctrl+Shift+V pastes as plain text (`EditCommandKind::PasteUnformatted`).
- Tab: inside a table, moves to the next cell (auto-inserts a row at
  the last cell); at the start of a list item, increases indent;
  otherwise inserts a literal tab. Shift+Tab is the inverse.
- Ctrl+Enter always inserts a block, bypassing the "Enter-in-table
  navigates to the cell below" behaviour.
- Backspace at the start of an indented list item dedents; at indent
  zero it exits the list.
- Shift+Arrow at a cell boundary activates rectangular cell selection;
  further Shift+Arrows extend the rectangle.
- Links and images are clickable — install callbacks via
  `.on_link_activated(...)` / `.on_image_activated(...)` on the editor.
- Right-click for Cut / Copy / Paste / Paste Unformatted / Select All.
  Item availability (Cut/Copy require a selection, Select All requires
  a non-empty document) refreshes on every open. Read-only preset ships
  a trimmed Copy + Select All variant. Apps that want to override
  pass their own factory via `RichTextEditor::context_menu(...)`.
- Ctrl+A single-shot select-all (the 4-level ladder is inside a table
  cell only — try this document's paragraphs and you'll see the
  single-shot behaviour).

## Not here yet

- IME composition (M10).
- RTF clipboard payload — the long-tail rich fallback for Pages /
  TextEdit / older Windows apps that don't emit HTML. HTML covers
  Firefox, Word, Google Docs, Apple Notes.

Type below, watch the preview update in real time.
"#;

fn main() {
    let doc = TextDocument::new();
    doc.set_markdown(SAMPLE)
        .expect("embedded markdown should parse");

    FernAppBuilder::new()
        .theme(Theme::light_default())
        .initial_window(
            WindowConfig::new()
                .title("FernUI — Rich Text Editor")
                .size(1100, 640)
                .root(move |tree, _state| {
                    let doc_editor = doc.clone();
                    let doc_preview = doc.clone();
                    let split = Signal::new(0.55);
                    tree.add(
                        SplitView::new(split)
                            .first(RichTextEditor::editor(doc_editor))
                            .second(
                                RichTextEditor::read_only(doc_preview)
                                    .v_scroll_policy(ScrollPolicy::Auto),
                            ),
                    )
                }),
        )
        .run();
}
