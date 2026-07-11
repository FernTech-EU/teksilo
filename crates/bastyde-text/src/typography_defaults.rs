// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Non-destructive per-editor default typography.
//!
//! [`EditorTypographyDefaults`] lets a host widget give a `RichTextEditor` a
//! *default* font family, line height, and first-line indent that apply only to
//! runs / blocks carrying **no explicit override** — the way a stylesheet
//! default applies until an inline style wins. These values are never written
//! back into the bound [`TextDocument`](text_document::TextDocument): they are
//! filled onto the disposable layout *snapshot* (`FlowSnapshot` / `BlockSnapshot`,
//! already detached from live document state) just before it is handed to the
//! typesetter. So they create no undo entry, never set `document.modified`, and
//! never touch a run / block the user actually authored.
//!
//! Size / zoom is deliberately **not** modelled here — it is a pure display
//! transform on the engine ([`RichTextEngine::set_zoom`](crate::RichTextEngine::set_zoom)),
//! not a per-run default fill.

use text_document::{BlockSnapshot, FlowElementSnapshot, FlowSnapshot, FragmentContent};

/// Per-editor default typography, applied to snapshot runs / blocks that carry
/// no explicit override. Cheap to clone; a default-valued instance is a no-op
/// (see the private `needs_snapshot_fill`) so editors that never set it pay
/// nothing.
#[derive(Debug, Clone, PartialEq)]
pub struct EditorTypographyDefaults {
    /// Fallback family for text runs whose `TextFormat.font_family` is `None`.
    /// `None` keeps the typesetter's own registry default (today's behavior).
    pub font_family: Option<String>,
    /// Line-height multiplier for blocks whose `BlockFormat.line_height` is
    /// `None`. `1.0` = normal single spacing (identical to the unset path).
    pub line_height: f32,
    /// First-line indent in logical px for body blocks whose
    /// `BlockFormat.text_indent` is `None`. Not applied to headings or list
    /// items. `0.0` = no indent (identical to the unset path).
    pub first_line_indent: f32,
    /// Space above each body paragraph, in logical px, for blocks whose
    /// `BlockFormat.top_margin` is `None`. Not applied to headings or list
    /// items. `0.0` = no extra space (identical to the unset path).
    pub paragraph_spacing_before: f32,
    /// Space below each body paragraph, in logical px, for blocks whose
    /// `BlockFormat.bottom_margin` is `None`. Not applied to headings or list
    /// items. `0.0` = no extra space (identical to the unset path).
    pub paragraph_spacing_after: f32,
}

impl Default for EditorTypographyDefaults {
    fn default() -> Self {
        Self {
            font_family: None,
            line_height: 1.0,
            first_line_indent: 0.0,
            paragraph_spacing_before: 0.0,
            paragraph_spacing_after: 0.0,
        }
    }
}

/// Whether `d` would change anything if filled onto a snapshot. When `false`,
/// the engine skips cloning the snapshot entirely, so every existing consumer
/// that never sets defaults pays zero cost.
pub(crate) fn needs_snapshot_fill(d: &EditorTypographyDefaults) -> bool {
    d.font_family.is_some()
        || (d.line_height - 1.0).abs() > f32::EPSILON
        || d.first_line_indent.abs() > f32::EPSILON
        || d.paragraph_spacing_before.abs() > f32::EPSILON
        || d.paragraph_spacing_after.abs() > f32::EPSILON
}

/// Fill `d`'s defaults into every block of an owned flow snapshot, recursing
/// into tables and sub-frames. Mutates the snapshot only — never the document.
pub(crate) fn apply_to_flow(flow: &mut FlowSnapshot, d: &EditorTypographyDefaults) {
    apply_to_elements(&mut flow.elements, d);
}

fn apply_to_elements(elements: &mut [FlowElementSnapshot], d: &EditorTypographyDefaults) {
    for el in elements {
        match el {
            FlowElementSnapshot::Block(b) => apply_to_block(b, d),
            FlowElementSnapshot::Table(t) => {
                for cell in &mut t.cells {
                    for b in &mut cell.blocks {
                        apply_to_block(b, d);
                    }
                }
            }
            FlowElementSnapshot::Frame(fr) => apply_to_elements(&mut fr.elements, d),
        }
    }
}

/// Fill `d`'s defaults into one block snapshot's unset fields.
pub(crate) fn apply_to_block(block: &mut BlockSnapshot, d: &EditorTypographyDefaults) {
    if block.block_format.line_height.is_none() {
        block.block_format.line_height = Some(d.line_height);
    }
    // First-line indent + paragraph spacing are body-paragraph conventions:
    // never applied to a heading or a list item (keeps titles and bullets on
    // their own rhythm, flush with the margin).
    let is_heading = block.block_format.heading_level.unwrap_or(0) != 0;
    let is_list_item = block.list_info.is_some();
    if !is_heading && !is_list_item {
        if block.block_format.text_indent.is_none() {
            block.block_format.text_indent = Some(d.first_line_indent.round() as i32);
        }
        if block.block_format.top_margin.is_none() {
            block.block_format.top_margin = Some(d.paragraph_spacing_before.round() as i32);
        }
        if block.block_format.bottom_margin.is_none() {
            block.block_format.bottom_margin = Some(d.paragraph_spacing_after.round() as i32);
        }
    }
    if let Some(family) = &d.font_family {
        for frag in &mut block.fragments {
            if let FragmentContent::Text { format, .. } = frag
                && format.font_family.is_none()
            {
                format.font_family = Some(family.clone());
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use text_document::{BlockFormat, BlockSnapshot, ListInfo, ListStyle, TextFormat};

    fn block(text: &str, format: TextFormat, block_format: BlockFormat) -> BlockSnapshot {
        let len = text.chars().count();
        BlockSnapshot {
            block_id: 0,
            position: 0,
            length: len,
            text: text.to_string(),
            fragments: vec![FragmentContent::Text {
                text: text.to_string(),
                format,
                offset: 0,
                length: len,
                element_id: 0,
                word_starts: Vec::new(),
            }],
            block_format,
            list_info: None,
            parent_frame_id: None,
            table_cell: None,
            paint_highlights: Vec::new(),
        }
    }

    fn defaults() -> EditorTypographyDefaults {
        EditorTypographyDefaults {
            font_family: Some("Literata".into()),
            line_height: 1.5,
            first_line_indent: 28.0,
            paragraph_spacing_before: 12.0,
            paragraph_spacing_after: 6.0,
        }
    }

    fn frag_family(b: &BlockSnapshot) -> Option<&str> {
        let FragmentContent::Text { format, .. } = &b.fragments[0] else {
            panic!("expected text fragment");
        };
        format.font_family.as_deref()
    }

    #[test]
    fn default_is_a_noop() {
        assert!(!needs_snapshot_fill(&EditorTypographyDefaults::default()));
    }

    #[test]
    fn any_set_field_requests_fill() {
        assert!(needs_snapshot_fill(&EditorTypographyDefaults {
            font_family: Some("X".into()),
            ..Default::default()
        }));
        assert!(needs_snapshot_fill(&EditorTypographyDefaults {
            line_height: 1.5,
            ..Default::default()
        }));
        assert!(needs_snapshot_fill(&EditorTypographyDefaults {
            first_line_indent: 24.0,
            ..Default::default()
        }));
        assert!(needs_snapshot_fill(&EditorTypographyDefaults {
            paragraph_spacing_before: 10.0,
            ..Default::default()
        }));
    }

    #[test]
    fn fills_unset_body_block() {
        let mut b = block("Hello", TextFormat::default(), BlockFormat::default());
        apply_to_block(&mut b, &defaults());
        assert_eq!(b.block_format.line_height, Some(1.5));
        assert_eq!(b.block_format.text_indent, Some(28));
        assert_eq!(b.block_format.top_margin, Some(12));
        assert_eq!(b.block_format.bottom_margin, Some(6));
        assert_eq!(frag_family(&b), Some("Literata"));
    }

    #[test]
    fn does_not_override_explicit_values() {
        let block_format = BlockFormat {
            line_height: Some(2.0),
            text_indent: Some(4),
            top_margin: Some(3),
            bottom_margin: Some(5),
            ..Default::default()
        };
        let format = TextFormat {
            font_family: Some("Courier".into()),
            ..Default::default()
        };
        let mut b = block("Hi", format, block_format);
        apply_to_block(&mut b, &defaults());
        assert_eq!(b.block_format.line_height, Some(2.0));
        assert_eq!(b.block_format.text_indent, Some(4));
        assert_eq!(b.block_format.top_margin, Some(3));
        assert_eq!(b.block_format.bottom_margin, Some(5));
        assert_eq!(frag_family(&b), Some("Courier"));
    }

    #[test]
    fn heading_gets_leading_and_font_but_not_indent() {
        let block_format = BlockFormat {
            heading_level: Some(1),
            ..Default::default()
        };
        let mut b = block("Title", TextFormat::default(), block_format);
        apply_to_block(&mut b, &defaults());
        assert_eq!(b.block_format.line_height, Some(1.5));
        assert_eq!(
            b.block_format.text_indent, None,
            "headings must not be first-line indented"
        );
        assert_eq!(
            b.block_format.top_margin, None,
            "headings must not get body paragraph spacing"
        );
        assert_eq!(frag_family(&b), Some("Literata"));
    }

    #[test]
    fn list_item_is_not_indented() {
        let mut b = block("Item", TextFormat::default(), BlockFormat::default());
        b.list_info = Some(ListInfo {
            list_id: 1,
            style: ListStyle::Disc,
            indent: 0,
            marker: "•".into(),
            item_index: 0,
        });
        apply_to_block(&mut b, &defaults());
        assert_eq!(
            b.block_format.text_indent, None,
            "list items must not be first-line indented"
        );
        assert_eq!(b.block_format.line_height, Some(1.5));
    }
}
