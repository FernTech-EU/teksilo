// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Where the links of a document are, for the keys and the accessibility walk.
//!
//! A link is a character format, so a link is the stretch of text runs that
//! agree about `anchor_href`, an inner mark (bold inside a link) splitting it
//! into several runs. This is the one place the editor finds those stretches:
//! the Tab stops of a viewer, the `Role::Link` nodes of the walk and the
//! context menu's "Open link" all read it, so a reader and a keyboard user
//! meet the same links. The rule is text-document's `link_at_caret`, which a
//! test holds it to: text runs only (an image or a footnote marker ends a
//! link), and two links that touch stay two links when their destinations
//! differ.

use teksilo_text::text_document::{BlockSnapshot, FlowElementSnapshot, FragmentContent};

/// One link of one block, in the block's own character space.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct BlockLink {
    /// First character, block-relative.
    pub(super) start: usize,
    /// One past the last character, block-relative.
    pub(super) end: usize,
    pub(super) href: String,
    /// The text the link covers.
    pub(super) text: String,
    /// The id of the link's first format run, which survives edits that do
    /// not delete it: what keys the link's accessibility nodes.
    pub(super) element_id: u64,
}

/// One link of the document, in the document's character space.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct DocLink {
    pub(super) start: usize,
    pub(super) end: usize,
    pub(super) href: String,
}

/// The links of `block`, in reading order.
pub(super) fn block_links(block: &BlockSnapshot) -> Vec<BlockLink> {
    let mut links: Vec<BlockLink> = Vec::new();
    let mut open = false;
    for fragment in &block.fragments {
        let FragmentContent::Text {
            text,
            format,
            offset,
            length,
            element_id,
            ..
        } = fragment
        else {
            // An image or a footnote marker is not link text: it ends one.
            open = false;
            continue;
        };
        let Some(href) = format.anchor_href.as_ref() else {
            open = false;
            continue;
        };
        if open
            && let Some(last) = links.last_mut()
            && last.href == *href
            && last.end == *offset
        {
            last.end = offset + length;
            last.text.push_str(text);
            continue;
        }
        links.push(BlockLink {
            start: *offset,
            end: offset + length,
            href: href.clone(),
            text: text.clone(),
            element_id: *element_id,
        });
        open = true;
    }
    links
}

/// Every link of the document, in document order: blocks, table cells (row
/// by row) and frames alike.
pub(super) fn document_links(elements: &[FlowElementSnapshot]) -> Vec<DocLink> {
    fn walk(elements: &[FlowElementSnapshot], out: &mut Vec<DocLink>) {
        for element in elements {
            match element {
                FlowElementSnapshot::Block(block) => push_block(block, out),
                FlowElementSnapshot::Table(table) => {
                    let mut cells: Vec<_> = table.cells.iter().collect();
                    cells.sort_by_key(|c| (c.row, c.column));
                    for cell in cells {
                        for block in &cell.blocks {
                            push_block(block, out);
                        }
                    }
                }
                FlowElementSnapshot::Frame(frame) => walk(&frame.elements, out),
            }
        }
    }
    fn push_block(block: &BlockSnapshot, out: &mut Vec<DocLink>) {
        out.extend(block_links(block).into_iter().map(|l| DocLink {
            start: block.position + l.start,
            end: block.position + l.end,
            href: l.href,
        }));
    }
    let mut out = Vec::new();
    walk(elements, &mut out);
    out
}

/// The link Tab moves to from a selection running `from..to`: the first one
/// starting at or after `to` that is not the selection itself. A link that
/// touches the selected one is the next stop, not skipped.
pub(super) fn next_link(links: &[DocLink], from: usize, to: usize) -> Option<&DocLink> {
    links
        .iter()
        .find(|l| l.start >= to && !(l.start == from && l.end == to))
}

/// The link Shift+Tab moves to: the last one ending at or before `from` that
/// is not the selection itself.
pub(super) fn previous_link(links: &[DocLink], from: usize, to: usize) -> Option<&DocLink> {
    links
        .iter()
        .rev()
        .find(|l| l.end <= from && !(l.start == from && l.end == to))
}
