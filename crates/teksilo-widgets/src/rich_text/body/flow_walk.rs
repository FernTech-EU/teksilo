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

use teksilo_canvas::{LineEnd, Point, Rect, TextGeometry};
use teksilo_core::accessibility::AccessNodeBuilder;
use teksilo_core::accessibility::text_runs::{TextRunSource, push_text_runs};
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

/// The text positions either side of a caret no block holds.
#[derive(Default, Clone, Copy)]
struct Nearest {
    /// The start of the first block after the caret.
    after: Option<(NodeId, usize)>,
    /// The end of the last block before it.
    before: Option<(NodeId, usize)>,
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
    caret: Option<(NodeId, usize)>,
    /// The run and character offset the selection anchor landed in.
    anchor: Option<(NodeId, usize)>,
    /// Where to report the caret and the anchor when no block holds them: a
    /// table's own positions before its first cell and after its last are
    /// positions the caret stops on (Right from the paragraph before a table
    /// takes two presses to reach the first cell), and no run has a character
    /// there. The start of the next text, else the end of the last one, so a
    /// reader is never left without a caret. See [`Self::selection`].
    nearest: [Nearest; 2],
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
            nearest: [Nearest::default(), Nearest::default()],
            syn_map: HashMap::new(),
            annotation_bodies: HashMap::new(),
        }
    }

    /// The `(anchor, caret)` to report: where each landed, or for one no
    /// block holds, the text nearest it.
    pub(super) fn selection(&self) -> (Option<(NodeId, usize)>, Option<(NodeId, usize)>) {
        let resolve = |landed: Option<(NodeId, usize)>, nearest: Nearest| {
            landed.or(nearest.after).or(nearest.before)
        };
        (
            resolve(self.anchor, self.nearest[1]),
            resolve(self.caret, self.nearest[0]),
        )
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
        // The separator after the block is a character of the document, one
        // position like any other, and a block snapshot's own text stops short
        // of it. AccessKit reads a paragraph's end from a `\n` ending the
        // line's last run (`is_paragraph_end`), so the separator goes there,
        // as one hard break per the emitter's contract. Without it a reader
        // got the paragraphs run together, and the end of one paragraph and
        // the start of the next were one offset. It also makes the source's
        // character space the document's, so a caret maps across with a
        // subtraction. The code editor's walk does the same.
        let mut text = block.text.clone();
        text.push('\n');
        let (block_left, block_top, mut lines) = self
            .st
            .engine
            .block_geometry(block.block_id, &block.text)
            .map(|(info, lines)| (info.x, info.y, lines))
            .unwrap_or((0.0, 0.0, Vec::new()));
        if let Some(last) = lines.last_mut() {
            last.byte_range.end = text.len();
            last.char_range.end += 1;
            last.end = LineEnd::HardBreak { chars: 1, bytes: 1 };
        }
        let origin = Point::new(
            block_left - self.scroll_x,
            (block_top - self.scroll_y) * self.zoom,
        );
        let mut source = if lines.is_empty() {
            // Not laid out: the text is still announced, and still ends its
            // paragraph, with a caret-shaped box rather than none at all.
            let mut flat = TextRunSource::flat(&text, block.block_id as u64);
            flat.lines[0].end = LineEnd::HardBreak { chars: 1, bytes: 1 };
            flat.with_fallback_rect(Rect::new(
                origin.x,
                origin.y,
                0.0,
                self.st.engine.default_line_height(),
            ))
        } else {
            let geometry = TextGeometry {
                lines,
                dropped_lines: 0,
                source_len: text.len(),
                rendered_text: None,
                links: Vec::new(),
            };
            TextRunSource::from_geometry(&text, &geometry, origin, block.block_id as u64)
        };

        // An inline object is one character of the document, its U+FFFC in
        // the block's text: an image, or a footnote's marker. A reader hears
        // it as its description, still one character, in its place in the
        // run. A run of its own after the block's text read the description
        // after the paragraph, and added a character the document does not
        // have.
        for frag in &block.fragments {
            let (description, offset) = match frag {
                FragmentContent::Image { alt, offset, .. } => (alt, *offset),
                FragmentContent::FootnoteReference { marker, offset, .. } => (marker, *offset),
                FragmentContent::Text { .. } => continue,
            };
            if let Some((byte, _)) = text.char_indices().nth(offset) {
                source.speak_as(byte, description.clone());
            }
        }

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
        for (index, user) in [self.user_pos, self.user_anchor].into_iter().enumerate() {
            let nearest = &mut self.nearest[index];
            if block.position > user && nearest.after.is_none() {
                nearest.after = emission.position_of(0);
            } else if block.position + block_chars < user {
                nearest.before = emission.position_of(block_chars).or(nearest.before);
            }
        }
    }
}
