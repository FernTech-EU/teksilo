// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Rich Text tab — RichTextEditor::editor (intrinsic min/max-lines
//! sizing, the messenger-composer pattern) + RichTextEditor::read_only.
//! Cannibalized from the `rich-text-editor` / `rich-text-viewer`
//! examples. Backed by the external `text-document` model.

use bastyde::prelude::*;
use bastyde::text_document::TextDocument;
use bastyde::widgets::rich_text::RichTextEditor;
use bastyde::widgets::{Divider, FontPicker, MaxSize, TextWidget, VStack};

use crate::shared::{Signals, section, tab_header};

pub fn title() -> LocalizedString {
    tr!(tab_richtext_title())
}

pub fn refs() -> LocalizedString {
    tr!(tab_richtext_refs())
}

const VIEWER_SAMPLE: &str = r#"# Read-only viewer

`RichTextEditor::read_only` renders a `TextDocument` without a caret.
It supports selection, mouse-wheel scrolling, and keyboard navigation
(arrows, Home/End, Ctrl+A) — but rejects every mutating key.

## Lists

- **Bold**, *italic*, and `inline code`
- Nested bullets reflow:
  - second level
  - second level again
- Crisp glyphs at any display DPI

1. Ordered lists too
2. With their own numbering
3. And wrapping

## Blockquote

> Blockquotes reflow and can nest.
>
> > Even a quote inside a quote.

## Table

| Widget    | Tier     | Crate            |
| --------- | -------- | ---------------- |
| Button    | control  | bastyde-widgets  |
| BarChart  | chart    | bastyde-charts   |
| SceneView | viewport | bastyde-scene    |
"#;

const EDITOR_SAMPLE: &str = r#"Type here — **bold**, *italic*, and `code` all work.

- editable bullet
- second bullet

> A blockquote you can edit.

| Knob      | Value |
| --------- | ----- |
| min_lines | 3     |
| max_lines | 10    |
"#;

fn editor_doc() -> TextDocument {
    let doc = TextDocument::new();
    let _ = doc.set_markdown(EDITOR_SAMPLE);
    doc
}

fn viewer_doc() -> TextDocument {
    let doc = TextDocument::new();
    let _ = doc.set_markdown(VIEWER_SAMPLE);
    doc
}

fn editor_widget() -> MaxSize {
    // Width is a cap, not a pin: the editor's `min_lines`/`max_lines`
    // already give it intrinsic height, so only the width axis needs
    // bounding — it fills up to 560 dp but shrinks into a narrow viewport.
    MaxSize::width(560.0_f32).child(
        RichTextEditor::editor(editor_doc())
            .min_lines(3)
            .max_lines(12),
    )
}

fn viewer_widget() -> MaxSize {
    // Unlike `editor_widget`, the read-only viewer has no min/max-lines
    // knob, so it stays in greedy sizing on both axes — the height cap
    // is still load-bearing (it scrolls internally rather than reporting
    // an unbounded height), but the width cap now shrinks with the
    // viewport instead of pinning at a literal 560 dp.
    MaxSize::new(560.0_f32, 240.0_f32).child(RichTextEditor::read_only(viewer_doc()))
}

/// A font-family picker: lists every installed font, previewing each name
/// next to a script-aware sample rendered in that font, and shows the
/// chosen family in its own typeface in the closed control.
fn font_picker_widget() -> MaxSize {
    // FontPicker is a ComboBox preset (flex-fill by default) — cap it
    // rather than pin it so it shrinks below 320 dp in a narrow viewport.
    MaxSize::width(320.0_f32).child(FontPicker::new(Signal::new(None::<String>)))
}

pub fn classic(ctx: &mut BuildContext, _sigs: &Signals) -> WidgetId {
    let header = tab_header(ctx, title(), refs());
    let editor = section(
        ctx,
        lit!("RichTextEditor::editor (min/max lines)"),
        editor_widget(),
    );
    let viewer = section(ctx, lit!("RichTextEditor::read_only"), viewer_widget());
    let font = section(
        ctx,
        lit!("FontPicker (choose a font family)"),
        font_picker_widget(),
    );

    ctx.add(
        VStack::new()
            .spacing(20.0)
            .add_child(header)
            .child(Divider::new())
            .add_child(editor)
            .add_child(viewer)
            .add_child(font),
    )
}

pub fn bati(ctx: &mut BuildContext, _sigs: &Signals) -> WidgetId {
    // RichTextEditor is built from a `TextDocument` constructor arg —
    // pre-build each and splice via `#{ id }`.
    let editor_id = ctx.add(editor_widget());
    let viewer_id = ctx.add(viewer_widget());
    let font_id = ctx.add(font_picker_widget());

    bati!(ctx => VStack {
            spacing: 20.0
            VStack {
                spacing: 4.0
                TextWidget::new(tr!(tab_richtext_title())) {
                    style: TextStyleRole::BodyBold
                    color: TextRole::Primary
                }
                TextWidget::new(tr!(tab_richtext_refs())) {
                    style: TextStyleRole::Small
                    color: TextRole::Secondary
                }
            }
            Divider

            VStack {
                spacing: 6.0
                TextWidget::new(lit!("RichTextEditor::editor (min/max lines)")) {
                    style: TextStyleRole::SmallBold
                    color: TextRole::Accent
                }
                #{ editor_id }
            }

            VStack {
                spacing: 6.0
                TextWidget::new(lit!("RichTextEditor::read_only")) {
                    style: TextStyleRole::SmallBold
                    color: TextRole::Accent
                }
                #{ viewer_id }
            }

            VStack {
                spacing: 6.0
                TextWidget::new(lit!("FontPicker (choose a font family)")) {
                    style: TextStyleRole::SmallBold
                    color: TextRole::Accent
                }
                #{ font_id }
            }
        }
    )
}
