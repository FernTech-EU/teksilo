// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! The accessibility walk over a rich-text document's flow.
//!
//! A document flow is three kinds of element, not one: blocks in the
//! document's own column, tables, and frames (a blockquote, a text frame).
//! Tables and frames nest, and both hold ordinary blocks inside them — so a
//! walk that handles only `FlowElementSnapshot::Block` does not merely lose
//! table *structure*, it loses every character of text those containers hold.
//! That is what this module exists to walk.
//!
//! Two constraints shape it, and both are properties of
//! `accesskit_consumer` rather than preferences:
//!
//! 1. **Runs are found at any depth.** A text-range container gathers its
//!    `Role::TextRun` descendants through `text_node_filter`, which excludes
//!    every non-run node it meets but keeps descending. So the editor's own
//!    text document still spans the whole flow — cells and blockquotes
//!    included — however deeply the runs are nested, and caret navigation
//!    crosses a table without anything special being done for it.
//!
//! 2. **A run's text-change event goes to its *filtered* parent.** The
//!    platform adapters reroute a changed run's event to
//!    `filtered_parent(&common_filter)`, and `common_filter` makes only
//!    `GenericContainer` and `TextRun` transparent. That parent is then asked
//!    whether it `supports_text_ranges()` — true only for a text input,
//!    `Label`, `Document` or `Terminal`. A `Role::Cell` answers no, so runs
//!    parented straight onto a cell would leave every keystroke typed into
//!    the table unannounced. Each cell therefore carries one `Role::Label`
//!    holding its runs, which answers yes.
//!
//! A blockquote frame earns a `Role::Blockquote` — a quotation is information
//! no run carries — and its blocks each get a container beneath it, because
//! `Role::Blockquote` is not text-range capable either. Every other frame is a
//! layout box (a positioned or floating text frame) with no role to announce,
//! so its content is emitted straight into the enclosing document rather than
//! behind a node a reader would stop on for nothing.
//!
//! Three structural roles therefore carry a `Role::Label` between themselves
//! and their runs — `Cell`, `Heading`, `Blockquote` — and the rule generalises:
//! **anything given a node between the editor and its text needs one.**

use std::collections::HashMap;

use teksilo_canvas::{Point, Rect, TextGeometry};
use teksilo_core::accessibility::text_runs::{TextRunSource, push_text_runs};
use teksilo_core::accessibility::{AccessNodeBuilder, TextRunAttributes, TextRunSpec};
use teksilo_core::accesskit::NodeId;
use teksilo_text::text_document::{
    BlockSnapshot, CellSnapshot, FlowElementSnapshot, FragmentContent, FrameSnapshot, TableSnapshot,
};

use super::state::{EditorState, SyntheticElementRef};

/// Packs a table id plus a row and a column into one element id, so a row and
/// a cell of the same table derive distinct — and stable — synthetic node ids.
///
/// The three are document-scoped counters; 21 bits each is far past any real
/// table and keeps the fields disjoint. A document that somehow exceeded that
/// would alias two nodes' ids, which `push_collected_child` drops with a
/// diagnostic rather than letting it panic the consumer's tree builder.
fn packed_element_id(table_id: usize, row: usize, column: usize) -> u64 {
    const MASK: u64 = (1 << 21) - 1;
    ((table_id as u64 & MASK) << 42) | ((row as u64 & MASK) << 21) | (column as u64 & MASK)
}

/// The mutable state carried across one accessibility walk of the flow.
pub(super) struct FlowWalk<'a> {
    st: &'a EditorState,
    zoom: f32,
    scroll_x: f32,
    scroll_y: f32,
    emits_children: bool,
    user_pos: usize,
    user_anchor: usize,
    /// The run and character offset the selection focus landed in.
    pub(super) caret: Option<(NodeId, usize)>,
    /// The run and character offset the selection anchor landed in.
    pub(super) anchor: Option<(NodeId, usize)>,
    /// Where each emitted run lives in the document, so an AT-initiated
    /// `SetTextSelection` can be resolved back to a document position.
    pub(super) syn_map: HashMap<NodeId, SyntheticElementRef>,
    /// One body node per annotation for the whole walk, not one per run it
    /// covers: `push_annotation_child` derives the id from the group alone,
    /// so a second push gives two children sharing a `NodeId`, which panics
    /// `accesskit_consumer`'s tree builder.
    annotation_bodies: HashMap<u64, NodeId>,
}

impl<'a> FlowWalk<'a> {
    pub(super) fn new(
        st: &'a EditorState,
        emits_children: bool,
        user_anchor: usize,
        user_pos: usize,
    ) -> Self {
        Self {
            zoom: st.engine.zoom(),
            scroll_x: st.scroll_x.get(),
            scroll_y: st.scroll_y.get(),
            st,
            emits_children,
            user_pos,
            user_anchor,
            caret: None,
            anchor: None,
            syn_map: HashMap::new(),
            annotation_bodies: HashMap::new(),
        }
    }

    /// Walk a run of flow elements.
    ///
    /// `parent` is the structural node new nodes attach under (`None` = the
    /// editor's own node). `cell_text` is a cell's shared text container, set
    /// only while walking that cell's blocks.
    pub(super) fn elements(
        &mut self,
        builder: &mut AccessNodeBuilder,
        elements: &[FlowElementSnapshot],
        parent: Option<NodeId>,
        cell_text: Option<NodeId>,
    ) {
        for elem in elements {
            match elem {
                FlowElementSnapshot::Block(block) => self.block(builder, block, parent, cell_text),
                FlowElementSnapshot::Table(table) => self.table(builder, table, parent),
                FlowElementSnapshot::Frame(frame) => self.frame(builder, frame, parent, cell_text),
            }
        }
    }

    /// Emit one frame.
    ///
    /// A blockquote earns a `Role::Blockquote` — it is a quotation, which is
    /// information no run carries and which a reader navigates and announces
    /// by. Every other frame is a layout box (a positioned or floating text
    /// frame) with no role to announce, so its contents are emitted straight
    /// into the enclosing document rather than behind a node that would be an
    /// object-navigation stop for nothing.
    ///
    /// The blockquote's own blocks each get a text container beneath it, for
    /// the reason in the module header — `Role::Blockquote` is not text-range
    /// capable, so runs hung straight off it would go silent on every edit.
    fn frame(
        &mut self,
        builder: &mut AccessNodeBuilder,
        frame: &FrameSnapshot,
        parent: Option<NodeId>,
        cell_text: Option<NodeId>,
    ) {
        if frame.format.is_blockquote != Some(true) {
            self.elements(builder, &frame.elements, parent, cell_text);
            return;
        }
        // The quote node replaces any inherited text container: its own blocks
        // each get one beneath it, so the structural node stays *above* the
        // text rather than below it. (`cell_text` cannot in fact be set here —
        // a `CellSnapshot` holds blocks and no frames — but dropping it is the
        // answer that stays right if that ever changes.)
        let Some(quote) = builder.push_blockquote_child(parent, frame.frame_id as u64) else {
            self.elements(builder, &frame.elements, parent, cell_text);
            return;
        };
        self.elements(builder, &frame.elements, Some(quote), None);
    }

    /// Emit one table: a `Role::Table` carrying its dimensions, a `Role::Row`
    /// per row, a `Role::Cell` per cell with its coordinates and spans, and
    /// each cell's content under the cell's own text node.
    ///
    /// Rows are emitted for `0..table.rows` rather than for the rows the
    /// cells happen to mention, so a row whose cells are all missing from the
    /// snapshot still occupies its index and the coordinates a reader is told
    /// keep matching the table it is being told the size of.
    fn table(
        &mut self,
        builder: &mut AccessNodeBuilder,
        table: &TableSnapshot,
        parent: Option<NodeId>,
    ) {
        let Some(table_node) =
            builder.push_table_child(parent, table.table_id as u64, table.rows, table.columns)
        else {
            return;
        };

        // Geometry is optional: a document that has not been laid out yet
        // still has to produce a correct *structural* tree, and a missing box
        // is better than a wrong one.
        let geom = self.st.engine.table_visual_info(table.table_id);
        if let Some(g) = &geom {
            builder
                .set_child_bounds_local(table_node, self.local_rect(g.x, g.y, g.width, g.height));
        }

        // A cell the snapshot places outside the dimensions it also declares
        // is a contradiction, not a shape to render: emitting it would put a
        // coordinate in the tree that points off the grid a reader was just
        // told the size of. Skipped — and said out loud in a debug build,
        // because the cost of skipping is that the cell's text goes missing,
        // which is the defect this whole walk exists to fix.
        debug_assert!(
            table
                .cells
                .iter()
                .all(|c| c.row < table.rows && c.column < table.columns),
            "table {} declares {}x{} but has a cell outside it; its text will \
             not reach the accessibility tree",
            table.table_id,
            table.rows,
            table.columns
        );

        for row in 0..table.rows {
            let Some(row_node) = builder.push_table_row_child(
                table_node,
                packed_element_id(table.table_id, row, 0),
                // 1-based: the builder's public ordinal convention.
                row + 1,
            ) else {
                continue;
            };
            if let Some(g) = &geom
                && let (Some(y), Some(h)) = (g.row_ys.get(row), g.row_heights.get(row))
            {
                builder
                    .set_child_bounds_local(row_node, self.local_rect(g.x, g.y + y, g.width, *h));
            }

            // Ordered by column explicitly. `TableSnapshot::cells` comes out
            // in the document's own cell order, which nothing in its type or
            // its docs promises is row-major — and the order cells are pushed
            // in *is* the order a screen reader reads the row, because the
            // editor's text document is its runs in tree order. Sorting here
            // costs a row's worth of indices and removes the dependency.
            let mut row_cells: Vec<_> = table
                .cells
                .iter()
                .filter(|c| c.row == row && c.column < table.columns)
                .collect();
            row_cells.sort_by_key(|c| c.column);

            for cell in row_cells {
                // `column + 1` keeps a cell's packed id clear of its row's,
                // which uses column 0. The two already differ by
                // `SyntheticKind`, so this is belt and braces — but it costs
                // an increment and removes any dependence on that.
                let cell_id = packed_element_id(table.table_id, cell.row, cell.column + 1);
                let Some(cell_node) = builder.push_table_cell_child(
                    row_node,
                    cell_id,
                    // 1-based, as above.
                    cell.row + 1,
                    cell.column + 1,
                    cell.row_span,
                    cell.column_span,
                ) else {
                    continue;
                };

                // A merged cell covers every track it spans. Sizing its box
                // from its origin track alone would leave the rest of the
                // area it visibly occupies pointing at nothing, so a
                // touch-explore or magnifier user aiming at the far half of a
                // merged cell would hit the gap between two boxes.
                let cell_rect = geom.as_ref().and_then(|g| self.span_rect(g, cell, table));
                if let Some(rect) = cell_rect {
                    builder.set_child_bounds_local(cell_node, rect);
                }

                // One text node per cell, not per block: it is the cell's
                // content that a reader announces and that a text-change
                // event is about, and a second node pushed for a second
                // paragraph would split both.
                let text_node = builder.push_cell_text_child(cell_node, cell_id);
                // The container is the node a reader announces as the cell's
                // content, so it needs the cell's box too — without one, a
                // `GetExtents` on it answers nothing.
                if let (Some(text_node), Some(rect)) = (text_node, cell_rect) {
                    builder.set_child_bounds_local(text_node, rect);
                }
                for block in &cell.blocks {
                    self.block(builder, block, Some(cell_node), text_node);
                }
            }
        }
    }

    /// The document-space rectangle a cell occupies, spanning every row and
    /// column track its `row_span` / `column_span` covers.
    fn span_rect(
        &self,
        g: &teksilo_text::TableVisualInfo,
        cell: &CellSnapshot,
        table: &TableSnapshot,
    ) -> Option<Rect> {
        let last_col =
            (cell.column + cell.column_span.max(1) - 1).min(table.columns.saturating_sub(1));
        let last_row = (cell.row + cell.row_span.max(1) - 1).min(table.rows.saturating_sub(1));
        let x0 = *g.column_xs.get(cell.column)?;
        let x1 = *g.column_xs.get(last_col)? + *g.column_content_widths.get(last_col)?;
        let y0 = *g.row_ys.get(cell.row)?;
        let y1 = *g.row_ys.get(last_row)? + *g.row_heights.get(last_row)?;
        Some(self.local_rect(g.x + x0, g.y + y0, x1 - x0, y1 - y0))
    }

    /// A document-space rectangle in the body's own coordinates — the space
    /// `set_child_bounds_local` expects.
    ///
    /// Deliberately the same convention the run path uses for its own origin:
    /// x loses the horizontal scroll, y loses the vertical scroll and is
    /// scaled by the zoom, and the extents are left alone. Applying the zoom
    /// to the extents here instead would make a cell's box disagree with the
    /// boxes of the runs inside it at any zoom but 1.
    fn local_rect(&self, x: f32, y: f32, width: f32, height: f32) -> Rect {
        Rect::new(
            x - self.scroll_x,
            (y - self.scroll_y) * self.zoom,
            width,
            height,
        )
    }

    /// Emit one block's text runs.
    ///
    /// `structural` is the node this block's own structure attaches under (a
    /// cell, a blockquote, or `None` for the editor); `cell_text` is a cell's
    /// shared text container, which takes precedence when set.
    fn block(
        &mut self,
        builder: &mut AccessNodeBuilder,
        block: &BlockSnapshot,
        structural: Option<NodeId>,
        cell_text: Option<NodeId>,
    ) {
        // One lookup for both: a block nested in a cell or a frame has to be
        // *found* first, and that search is a walk rather than a hash lookup.
        //
        // A nested block also has its own left origin — column 1 of a table
        // does not start at the document's left edge — so the horizontal
        // origin comes from the block rather than being assumed to be zero.
        // It *is* zero for a block in the document's own column, which is what
        // keeps this identical to the plain-prose case.
        let (block_left, block_top, lines) = self
            .st
            .engine
            .block_geometry(block.block_id, &block.text)
            .map(|(info, lines)| (info.x, info.y, lines))
            .unwrap_or((0.0, 0.0, Vec::new()));
        let geometry = TextGeometry {
            lines,
            dropped_lines: 0,
            source_len: block.text.len(),
            rendered_text: None,
            links: Vec::new(),
        };
        let origin = Point::new(
            block_left - self.scroll_x,
            (block_top - self.scroll_y) * self.zoom,
        );
        let source =
            TextRunSource::from_geometry(&block.text, &geometry, origin, block.block_id as u64);

        // Where the runs hang, in precedence order.
        //
        // A cell's shared container wins outright: a cell's runs have to hang
        // off it or every keystroke typed into the cell is dropped before it
        // reaches a screen reader (see `SyntheticKind::RichTextCellText`), and
        // a `Role::Heading` in between would reintroduce exactly that. A
        // heading in a table cell therefore reads as ordinary cell text.
        //
        // Otherwise a heading keeps a node of its own — it is how a reader
        // jumps through a document, and it carries a level no run does — with
        // a text container beneath it, because `Role::Heading` is not
        // text-range capable either. A block directly inside a blockquote gets
        // that same container under the quote node, for the same reason.
        let parent = if let Some(cell_text) = cell_text {
            Some(cell_text)
        } else if !self.emits_children {
            None
        } else if let Some(level) = block.block_format.heading_level {
            let heading = builder.push_paragraph_child(block.block_id as u64);
            builder.set_paragraph_as_heading(heading, level);
            builder
                .push_block_text_child(heading, block.block_id as u64)
                .or(Some(heading))
        } else if let Some(structural) = structural {
            builder
                .push_block_text_child(structural, block.block_id as u64)
                .or(Some(structural))
        } else {
            None
        };

        let emission = push_text_runs(builder, parent, &source);

        for run in &emission.runs {
            let absolute_start = block.position + run.char_range.start;
            let absolute_end = block.position + run.char_range.end;

            // Remember where this run lives in the document so the
            // on-access handler can resolve
            // SetTextSelection(TextRun NodeId, char_index).
            self.syn_map.insert(
                run.id,
                SyntheticElementRef {
                    element_id: block.block_id as u64,
                    absolute_start,
                    text: source
                        .text
                        .get(run.byte_range.clone())
                        .unwrap_or_default()
                        .to_string(),
                },
            );

            // Annotations covering this run: one Role::Comment node
            // each, linked from the run through `details`. Linked per
            // run rather than once per span because a span can cross
            // runs (a wrapped sentence splits at every line), and every
            // covered run must carry the relation or the announcement
            // drops out halfway through the phrase.
            for span in &self.st.annotation_spans {
                if span.start < absolute_end && span.end > absolute_start {
                    let body = match self.annotation_bodies.get(&span.group_id) {
                        Some(body) => *body,
                        None => {
                            let body =
                                builder.push_annotation_child(span.group_id, span.summary.clone());
                            self.annotation_bodies.insert(span.group_id, body);
                            body
                        }
                    };
                    builder.push_detail_on_child(run.id, body);
                }
            }
        }

        // Resolve the user's cursor / anchor against this block. The
        // emitter knows which run a character landed in, including the
        // chunk splits it made at 255 characters, so no call site has
        // to reproduce that arithmetic.
        let block_chars = block.text.chars().count();
        if self.user_pos >= block.position && self.user_pos <= block.position + block_chars {
            self.caret = emission
                .position_of(self.user_pos - block.position)
                .or(self.caret);
        }
        if self.user_anchor >= block.position && self.user_anchor <= block.position + block_chars {
            self.anchor = emission
                .position_of(self.user_anchor - block.position)
                .or(self.anchor);
        }

        // Inline objects: one document character each, rendered as
        // something a reader sees but cannot read out of the text — an
        // image, or a footnote's marker.
        //
        // Announced as a single-character text run whose value is that
        // description. `character_lengths` is one entry spanning the
        // whole string on purpose: the object *is* one character of the
        // document, however many letters stand in for it, and telling
        // AccessKit otherwise would put every caret offset after it out
        // by the difference.
        for frag in &block.fragments {
            let object_run = match frag {
                FragmentContent::Image {
                    alt,
                    offset,
                    element_id,
                    format,
                    ..
                } => Some((alt.clone(), *offset, *element_id, format)),
                FragmentContent::FootnoteReference {
                    marker,
                    offset,
                    element_id,
                    format,
                    ..
                } => Some((marker.clone(), *offset, *element_id, format)),
                FragmentContent::Text { .. } => None,
            };
            let Some((value, offset, element_id, format)) = object_run else {
                continue;
            };
            if !self.emits_children {
                continue;
            }

            // Text attributes for AT (WCAG 1.3.1 / EN 301 549
            // 11.5.2.9). AccessKit has no bold flag, so an explicit
            // weight wins, else bold folds to 700.
            let attrs = TextRunAttributes {
                font_weight: format.font_weight.map(|w| w as u16),
                bold: format.font_bold.unwrap_or(false),
                italic: format.font_italic.unwrap_or(false),
                underline: format.font_underline.unwrap_or(false),
                strikethrough: format.font_strikeout.unwrap_or(false),
            };

            // An empty description would announce nothing at all, which
            // is indistinguishable from a rendering fault. A single
            // space is at least a spoken pause. The length rides in a
            // `u8`, so a description longer than that loses its tail
            // rather than making AccessKit's own invariant unsatisfiable.
            let mut value = if value.is_empty() {
                " ".to_string()
            } else {
                value
            };
            while value.len() > u8::MAX as usize {
                value.pop();
            }

            // Geometry for the one character it occupies, anchored on
            // the line it sits in: a run with no bounds empties
            // `bounding_boxes()` for every range that touches it.
            let byte_offset = block
                .text
                .char_indices()
                .nth(offset)
                .map(|(index, _)| index)
                .unwrap_or(block.text.len());
            let line = source
                .lines
                .iter()
                .find(|line| line.byte_range.contains(&byte_offset))
                .or_else(|| source.lines.last());
            let glyph = self
                .st
                .engine
                .character_geometry(block.block_id, offset, offset + 1);
            let (position, width) = glyph
                .first()
                .map(|g| (g.position, g.width))
                .unwrap_or((0.0, 0.0));
            let bounds = match line {
                Some(line) => {
                    Rect::new(line.rect.x + position, line.rect.y, width, line.rect.height)
                }
                None => Rect::new(origin.x + position, origin.y, width, 0.0),
            };

            let Some(node_id) = builder.push_text_run(
                parent,
                TextRunSpec {
                    element_id,
                    character_lengths: vec![value.len() as u8],
                    word_starts: vec![0],
                    character_positions: vec![0.0],
                    character_widths: vec![width],
                    value: value.clone(),
                    bounds,
                    bounds_are_absolute: false,
                    text_direction: source.base_direction,
                    attrs,
                },
            ) else {
                continue;
            };

            let absolute_start = block.position + offset;
            self.syn_map.insert(
                node_id,
                SyntheticElementRef {
                    element_id,
                    absolute_start,
                    text: value,
                },
            );
            if self.user_pos >= absolute_start && self.user_pos <= absolute_start + 1 {
                self.caret = Some((node_id, self.user_pos - absolute_start));
            }
            if self.user_anchor >= absolute_start && self.user_anchor <= absolute_start + 1 {
                self.anchor = Some((node_id, self.user_anchor - absolute_start));
            }
        }
    }
}
