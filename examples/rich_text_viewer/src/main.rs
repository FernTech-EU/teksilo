// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Rich text viewer demo.
//!
//! M8a demo of `RichTextEditor::read_only`. Loads a markdown sample
//! into a `TextDocument` and displays it in a read-only viewer. Run
//! with: `cargo run -p rich-text-viewer`.
//!
//! This example is deliberately minimal — it exercises the widget's
//! layout, paint, and `on_change` event plumbing end to end without
//! depending on a file on disk.

use bastyde::prelude::*;
use bastyde::text_document::TextDocument;
use bastyde::widgets::rich_text::RichTextEditor;
use bastyde::widgets::{Expand, HStack, Spacer, Toolbar};

const SAMPLE: &str = r#"# Bastyde Rich Text Viewer

This window holds a single `RichTextEditor::read_only` bound to a
`TextDocument` loaded from an embedded markdown string. It is the
live target of Milestone 8a of §27.10 of the Bastyde architecture.

## What works today

- Crisp glyph rendering at any display DPI (the engine rasterizes
  at `zoom * display_scale_factor` and the paint walker divides
  screen coords back down, so nearest-neighbor atlas sampling still
  produces pixel-perfect text on HiDPI).
- The editor shares the application's `SharedTypesetter`, so its
  glyphs land in the same atlas bastyde-render uploads to the GPU.
- Mouse wheel scrolling.
- Click to place the caret, Shift+click to extend the selection.
- Arrow keys for character navigation, Ctrl+Arrow for word jumps,
  Home / End for start and end of visual line, Ctrl+Home /
  Ctrl+End for document ends, Page Up / Page Down for viewport
  paging, Shift+any of the above to extend the selection,
  Ctrl+A to select everything.
- Up and Down use a sticky preferred column so they keep trying
  to land on the same visual X across short lines.
- No caret: view-only widgets don't expose one. The editable
  preset (`RichTextEditor::editor`) is the one with a blinking
  caret, reached by M8b and not yet usable from this example.
- Multiple editors can bind to one `TextDocument`: each one
  subscribes to `on_change` independently, so edits propagate to
  every view.

## What's not here yet

- Editing: typing, backspace, enter, formatting, undo/redo, paste.
  These all come in Milestone 8b via the `RichTextEditor::editor`
  preset on top of the same shared modules.
- Tables and frames still render via the base (non-HiDPI-aware)
  path until the scaling helper is extended to cover them.
- Full rich clipboard round-trip (in-process rich fragment +
  plain-text fallback) — plain copy works, cut and paste are
  gated by the `ReadOnly` clipboard policy.

## Try it

Scroll with the mouse wheel. Click anywhere to place the (invisible)
caret. Hold the arrow keys to move the selection anchor through the
text — Shift+click and Shift+arrows extend a visible selection you
can then Ctrl+C (no-op until M8b) or Ctrl+A to select everything.
None of the keys that would mutate the document (typing, Enter,
Delete) do anything — the `CommandFilter::ReadOnly` rejects them
before they reach the cursor.

Tables and sub-frames aren't exercised here yet — the HiDPI
scaling path covers blocks only in M8a, and a verified fix for
tables needs test hardware with a range of DPIs.
"#;

fn dark_mode_toolbar() -> impl Widget {
    Toolbar::new().child(
        HStack::new()
            .child(Spacer::new())
            .child(bastyde::widgets::ThemeSwitcher::new()),
    )
}

fn main() {
    let doc = TextDocument::new();
    doc.set_markdown(SAMPLE)
        .expect("failed to parse embedded markdown");

    BastydeAppBuilder::new()
        .install_automation_bridge_in_debug()
        .install_inspector_in_debug()
        .theme(bastyde::presets::intui::light())
        .initial_window(
            WindowConfig::new()
                .title("Bastyde — Rich Text Viewer")
                .size(720, 540)
                .root(move |tree, _state| {
                    tree.add(
                        bastyde::widgets::VStack::new()
                            .child(dark_mode_toolbar())
                            .child(Expand::new().child(RichTextEditor::read_only(doc.clone()))),
                    )
                }),
        )
        .run();
}
