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
//!   * Ctrl+C / Ctrl+X / Ctrl+V round-trip rich content in-process;
//!     external paste lands as plain text.
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
  preserves rich formatting (bold runs, headings) via a stored
  `DocumentFragment`; external paste lands as plain text.
- Ctrl+A single-shot select-all (the 4-level ladder is inside a table
  cell only — try this document's paragraphs and you'll see the
  single-shot behaviour).

## Not here yet

- IME composition (M10).
- Platform-native rich clipboard MIME types (RTF / text/html).
- Menu bars and context menus — the editor exposes
  `context_target_at(point)` for the host application to build its
  own menus.

Type below, watch the preview update in real time.
"#;

fn main() {
    let doc = TextDocument::new();
    doc.set_markdown(SAMPLE)
        .expect("embedded markdown should parse");

    FernAppBuilder::new()
        .theme(Theme::light_default())
        .window_title("FernUI — Rich Text Editor")
        .window_size(1100, 640)
        .root(move |tree| {
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
        })
        .run();
}
