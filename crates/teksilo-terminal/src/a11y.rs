// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Accessibility for the terminal view.
//!
//! The terminal exposes its visible screen as a native `Role::Terminal` node
//! whose children are one run of `Role::TextRun` per visible row (so a screen
//! reader can review the screen with its normal text-navigation commands), with
//! the VT cursor mapped to the AT caret. The runs are built by
//! [`teksilo_core::accessibility::text_runs`], which owns the 255-character cap,
//! the UAX #29 word starts and the `next_on_line` chaining; one
//! [`TextRunSource`] per row, seeded with the row index so two identical rows
//! cannot collide on a node id.
//!
//! The runs are **direct** children of the terminal node. A `Role::Paragraph` in
//! between would be an object-navigation stop of its own — `common_filter` does
//! not exclude it — and it would break text-change events on all three
//! platforms, because a run's update routes to its filtered parent and a
//! paragraph supports no text ranges.
//!
//! Row rects are widget-local: the grid's offset inside the widget plus the
//! measured cell metrics. `AccessNodeBuilder::build` translates them into window
//! space once the walker has written the terminal's own box.
//!
//! New output is announced through a separate, small `Role::Status` live region
//! ([`LiveAnnouncer`]) rather than by re-announcing the whole screen — the way
//! screen readers actually consume ARIA live regions.

use accesskit::{Action, Live, Role, TextDirection};
use teksilo_canvas::{CharGeom, LineEnd, Point, Rect, SizeProposal};
use teksilo_core::accessibility::AccessNodeBuilder;
use teksilo_core::accessibility::text_runs::{
    SourceLine, SourceSegment, TextRunSource, push_text_runs,
};
use teksilo_core::binding::BindingLevel;
use teksilo_core::build_context::BuildContext;
use teksilo_core::signal::Signal;
use teksilo_core::widget::{LayoutContext, LayoutResponse, Widget};
use teksilo_core::widget_id::WidgetId;

use crate::engine::GridSnapshot;
use crate::render::CellMetrics;

/// Build the `Role::Terminal` accessibility subtree for the current screen.
///
/// `origin` is the top-left of the grid's content area **relative to the
/// terminal's own bounds**, so every emitted rect stays widget-local.
pub(crate) fn build_terminal_a11y(
    builder: &mut AccessNodeBuilder,
    snapshot: &GridSnapshot,
    metrics: CellMetrics,
    origin: Point,
    name: &str,
) {
    builder.set_role(Role::Terminal);
    if !name.is_empty() {
        builder.set_name(name);
    }
    builder.add_action(Action::Focus);
    builder.add_action(Action::ScrollUp);
    builder.add_action(Action::ScrollDown);
    // The terminal is a `keyboard_capture` surface: Tab is encoded to the
    // child, so the one chord that moves focus out is Ctrl+Tab. Announce it,
    // because a screen-reader user has no other way to discover that pressing
    // Tab here will not do what it does everywhere else in the app.
    builder.set_keyboard_shortcut("Ctrl+Tab");

    let mut cursor_target: Option<(accesskit::NodeId, usize)> = None;

    for row in 0..snapshot.screen_lines {
        let cursor_here = snapshot.cursor.visible && snapshot.cursor.line == row;
        let laid_out = row_layout(snapshot, row, metrics.width);

        let line_rect = Rect::new(
            origin.x,
            origin.y + row as f32 * metrics.height,
            snapshot.columns as f32 * metrics.width,
            metrics.height,
        );
        // A row with no text still reports a caret box, and on the cursor's row
        // that box has to sit under the block cursor rather than at the left
        // margin — a magnifier following the caret would otherwise jump to the
        // start of the line on every blank prompt line.
        let caret_x = if cursor_here {
            origin.x + snapshot.cursor.column as f32 * metrics.width
        } else {
            line_rect.x
        };

        let mut source = TextRunSource::flat(&laid_out.text, row as u64)
            .with_base_direction(TextDirection::LeftToRight);
        source.lines = vec![SourceLine {
            byte_range: 0..laid_out.text.len(),
            rect: line_rect,
            caret_x,
            // Rows are separate sources, and a reader tells them apart by the
            // `next_on_line` chain rather than by a newline character. Emitting
            // a break here would put characters in the accessible text that the
            // grid does not hold.
            end: LineEnd::EndOfText,
            segments: if laid_out.characters.is_empty() {
                Vec::new()
            } else {
                vec![SourceSegment {
                    byte_range: 0..laid_out.text.len(),
                    rect: Rect::new(line_rect.x, line_rect.y, laid_out.width, line_rect.height),
                    direction: TextDirection::LeftToRight,
                    characters: laid_out.characters,
                }]
            },
            truncation: None,
        }];

        let emission = push_text_runs(builder, None, &source);
        if cursor_here {
            // A row wider than the 255-character cap is several runs, so the
            // caret's run is whichever chunk holds its character.
            cursor_target = emission.position_of(laid_out.cursor_char);
        }
    }

    // Map the VT cursor to the AT caret (a collapsed selection).
    if let Some((node, idx)) = cursor_target {
        builder.set_text_selection_to((node, idx), (node, idx));
    }
}

/// One row flattened into text plus the geometry of the cells that produced it.
struct RowLayout {
    /// The row's cells concatenated, with trailing blanks trimmed so a reader
    /// is not read a line of spaces.
    text: String,
    /// One entry per character of `text`, positioned from the row's leading
    /// edge.
    characters: Vec<CharGeom>,
    /// How far the text extends from that edge.
    width: f32,
    /// Character index of the cursor's column within `text`.
    cursor_char: usize,
}

fn row_layout(snapshot: &GridSnapshot, row: usize, cell_width: f32) -> RowLayout {
    let mut text = String::with_capacity(snapshot.columns);
    let mut characters: Vec<CharGeom> = Vec::with_capacity(snapshot.columns);
    let mut cursor_char = None;

    for col in 0..snapshot.columns {
        if col == snapshot.cursor.column {
            cursor_char = Some(characters.len());
        }
        let Some(cell) = snapshot.cell(row, col) else {
            continue;
        };
        // A wide glyph contributes its text once, from its leading cell.
        if cell.attrs.wide_spacer {
            continue;
        }
        let advance = cell_width * if cell.attrs.wide { 2.0 } else { 1.0 };
        let x = col as f32 * cell_width;
        for (index, ch) in cell.text().chars().enumerate() {
            // A combining mark takes no advance of its own; it belongs at the
            // trailing edge of the base character it decorates, which is where
            // a caret placed after it sits.
            let geometry = if index == 0 {
                CharGeom {
                    position: x,
                    width: advance,
                }
            } else {
                CharGeom {
                    position: x + advance,
                    width: 0.0,
                }
            };
            characters.push(geometry);
            text.push(ch);
        }
    }

    let trimmed = text.trim_end().len();
    text.truncate(trimmed);
    characters.truncate(text.chars().count());

    let width = characters
        .last()
        .map(|c| c.position + c.width)
        .unwrap_or(0.0);
    // A cursor parked past the row's last non-blank column — the ordinary state
    // at a prompt — reports the end of the text rather than nothing.
    let cursor_char = cursor_char.unwrap_or(usize::MAX).min(characters.len());

    RowLayout {
        text,
        characters,
        width,
        cursor_char,
    }
}

/// A zero-size child node that carries the terminal's "new output" live region.
/// The terminal updates [`Self::text`] with each newly-completed output line,
/// and the platform adapters announce it as the node's name changes.
#[derive(Debug)]
pub(crate) struct LiveAnnouncer {
    pub(crate) text: Signal<String>,
}

impl Widget for LiveAnnouncer {
    fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
        // Re-walk this node's AT (only) whenever the announced text changes, so
        // a new completed line is picked up without a rebuild.
        self.text.bind_to(
            ctx.self_id(),
            ctx.binding_registry(),
            BindingLevel::AccessibilityOnly,
        );
        Vec::new()
    }

    fn layout_response(&self, _proposal: SizeProposal, _ctx: &LayoutContext) -> LayoutResponse {
        LayoutResponse::ZERO
    }

    fn accessibility(&self, builder: &mut AccessNodeBuilder) {
        builder.set_role(Role::Status);
        builder.set_live(Live::Polite);
        let text = self.text.get();
        if !text.is_empty() {
            // The name, not the value. Every AccessKit adapter announces a live
            // node's name, and `accesskit_consumer` takes a name from the value
            // for a `Role::Label` only (`node.rs:744-746`), so a `Status`
            // holding its line as a value was silent on all three platforms.
            builder.set_name(text);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::{Cell, CursorInfo, TermCursorShape};

    /// A completed line of output is announced, by its words. The announcer
    /// used to hold the line as a value, which no platform announces for a
    /// `Status`; the announcement ring used to read the value and record it
    /// anyway. It now replays the adapters' own rules, so it hears what they
    /// would.
    #[test]
    fn a_completed_line_is_announced() {
        let text = Signal::new(String::new());
        let mut tree = teksilo_core::widget_tree::WidgetTree::new();
        tree.add(LiveAnnouncer { text: text.clone() });
        tree.layout(SizeProposal::exact(400.0, 300.0));
        let _ = tree.sync_accessibility();
        let seen = tree.announcements_since(0).last().map_or(0, |a| a.seq);

        text.set("total 12".to_string());
        tree.layout(SizeProposal::exact(400.0, 300.0));
        let _ = tree.sync_accessibility();
        let heard: Vec<String> = tree
            .announcements_since(seen)
            .into_iter()
            .map(|a| a.text)
            .collect();
        assert_eq!(heard, vec!["total 12"]);
    }

    const METRICS: CellMetrics = CellMetrics {
        width: 8.0,
        height: 16.0,
    };
    /// The grid's offset inside the widget, so a rect that forgot to be
    /// widget-local would be off by it.
    const ORIGIN: Point = Point { x: 2.0, y: 3.0 };

    fn snapshot(rows: &[&str], columns: usize, cursor: (usize, usize)) -> GridSnapshot {
        let mut cells = vec![Cell::default(); columns * rows.len()];
        for (row, text) in rows.iter().enumerate() {
            for (col, ch) in text.chars().take(columns).enumerate() {
                cells[row * columns + col].ch = ch;
            }
        }
        GridSnapshot {
            columns,
            screen_lines: rows.len(),
            cells,
            cursor: CursorInfo {
                line: cursor.0,
                column: cursor.1,
                shape: TermCursorShape::Block,
                visible: true,
            },
            selection: None,
            display_offset: 0,
            history_len: 0,
        }
    }

    /// The terminal node and its emitted run children, as the walker would
    /// hand them to a platform adapter.
    fn emitted(
        snapshot: &GridSnapshot,
    ) -> (accesskit::Node, Vec<(accesskit::NodeId, accesskit::Node)>) {
        let owner = WidgetId::default();
        let mut builder = AccessNodeBuilder::for_widget(owner);
        build_terminal_a11y(&mut builder, snapshot, METRICS, ORIGIN, "Shell");
        let (_, node, children, _) = builder.build(owner);
        (node, children)
    }

    #[test]
    fn a_cursor_past_255_columns_lands_in_the_second_chunk() {
        // A run may carry at most 255 characters, so a wide terminal splits
        // each row. The caret is reported in source columns and has to be
        // resolved against the chunk that actually holds it — reporting
        // character 260 of a 255-character run makes the consumer probe an
        // index its `u8` cannot express.
        let row = "a".repeat(300);
        let snap = snapshot(&[&row], 300, (0, 260));
        let (node, children) = emitted(&snap);

        assert_eq!(children.len(), 2, "a 300-column row is two runs");
        assert_eq!(children[0].1.character_lengths().len(), 255);
        assert_eq!(children[1].1.character_lengths().len(), 45);

        let selection = node
            .text_selection()
            .expect("the VT cursor must reach the AT caret");
        assert_eq!(selection.focus.node, children[1].0);
        assert_eq!(selection.focus.character_index, 5);
        assert_eq!(selection.anchor.node, selection.focus.node);
        assert_eq!(
            selection.anchor.character_index,
            selection.focus.character_index
        );
    }

    #[test]
    fn every_row_reports_a_bounding_box() {
        // `Range::bounding_boxes()` discards every box it has already
        // collected the moment one run lacks bounds, a direction, positions
        // or widths — so a single row reporting none empties the geometry of
        // every range that crosses it, and a magnifier stops tracking the
        // whole screen rather than one line. The all-blank row is the case
        // with nothing to measure.
        let snap = snapshot(&["hi", "   ", "x"], 10, (2, 1));
        let (_, children) = emitted(&snap);

        assert_eq!(children.len(), 3, "one run per visible row");
        for (row, (_, node)) in children.iter().enumerate() {
            let bounds = node.bounds().expect("a row reported no bounds");
            assert_eq!(bounds.y0, ORIGIN.y as f64 + row as f64 * 16.0);
            assert_eq!(bounds.y1, bounds.y0 + 16.0);
            assert_eq!(bounds.x0, ORIGIN.x as f64);
            assert!(node.character_positions().is_some());
            assert!(node.character_widths().is_some());
            assert!(node.text_direction().is_some());
        }
        // Trailing blanks are trimmed, so the first row is two cells wide and
        // the blank row is the bare caret.
        assert_eq!(children[0].1.value(), Some("hi"));
        assert_eq!(children[1].1.value(), Some(""));
    }

    #[test]
    fn runs_are_direct_children_of_the_terminal_node() {
        // `accesskit_consumer::common_filter` does not exclude
        // `Role::Paragraph`, so a paragraph between the terminal and its runs
        // would be an object-navigation stop of its own — and a run's change
        // event, which routes to its filtered parent, would land on a node
        // supporting no text ranges and be dropped on all three platforms.
        let snap = snapshot(&["hi"], 10, (0, 0));
        let (node, children) = emitted(&snap);
        assert!(
            children
                .iter()
                .all(|(_, child)| child.role() == accesskit::Role::TextRun),
            "the terminal emitted something other than runs"
        );
        let run_ids: Vec<accesskit::NodeId> = children.iter().map(|(id, _)| *id).collect();
        for id in run_ids {
            assert!(node.children().contains(&id));
        }
    }

    #[test]
    fn a_wide_glyph_spans_two_columns_and_is_read_once() {
        // The trailing half of a double-width glyph carries no text of its
        // own; counting it would double the character and put every later
        // caret position one cell to the left of the cell it names.
        let mut snap = snapshot(&["ab"], 4, (0, 3));
        snap.cells[0].ch = '漢';
        snap.cells[0].attrs.wide = true;
        snap.cells[1].ch = ' ';
        snap.cells[1].attrs.wide_spacer = true;
        snap.cells[2].ch = 'z';

        let (node, children) = emitted(&snap);
        assert_eq!(children[0].1.value(), Some("漢z"));
        assert_eq!(
            children[0].1.character_widths(),
            Some(&[16.0f32, 8.0][..]),
            "the wide glyph must advance by two cells"
        );
        // Column 3 is past the row's text, so the caret sits at its end.
        let selection = node.text_selection().expect("no caret");
        assert_eq!(selection.focus.character_index, 2);
    }
}
