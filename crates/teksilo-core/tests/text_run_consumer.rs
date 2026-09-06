// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! What an assistive technology actually gets, measured through
//! `accesskit_consumer`.
//!
//! Every platform adapter answers its text API through this consumer, so
//! it — not the framework's own view of what it emitted — is the arbiter
//! of whether a label can be reviewed. Three of these assertions cannot be
//! made any other way: `supports_text_ranges` is a consumer method,
//! `document_range().text()` is what a reader reviews rather than what the
//! node announces, and `bounding_boxes()` is where a single geometry-less
//! run silently empties the geometry of an entire label.

use std::ops::Range as ByteRange;

use accesskit::{NodeId, Role, TreeUpdate};
use accesskit_consumer::{NodeRef, Tree};
use teksilo_canvas::{
    CharGeom, LineEnd, Rect, Size, SizeProposal, TextDirection, TextGeometry, TextLine,
    TextLineSegment,
};
use teksilo_core::accessibility::AccessNodeBuilder;
use teksilo_core::accessibility::text_runs::{TextRunSource, push_text_runs};
use teksilo_core::widget::{LayoutContext, LayoutResponse, Widget};
use teksilo_core::widget_tree::WidgetTree;

/// Fixed 8 dp per character, one line — the same metrics the mock text
/// backend uses, so a rectangle here is checkable by hand.
const CHAR_WIDTH: f32 = 8.0;
const LINE_HEIGHT: f32 = 16.0;

fn measured(text: &str) -> TextGeometry {
    let mut position = 0.0f32;
    let characters: Vec<CharGeom> = text
        .chars()
        .map(|_| {
            let geometry = CharGeom {
                position,
                width: CHAR_WIDTH,
            };
            position += CHAR_WIDTH;
            geometry
        })
        .collect();
    let count = characters.len();
    let byte_range: ByteRange<usize> = 0..text.len();
    TextGeometry {
        lines: vec![TextLine {
            index: 0,
            byte_range: byte_range.clone(),
            char_range: 0..count,
            rect: [0.0, 0.0, position, LINE_HEIGHT],
            baseline: LINE_HEIGHT * 0.75,
            caret_x: 0.0,
            segments: vec![TextLineSegment {
                byte_range,
                char_range: 0..count,
                direction: TextDirection::LeftToRight,
                rect: [0.0, 0.0, position, LINE_HEIGHT],
                characters,
            }],
            end: LineEnd::EndOfText,
            truncation: None,
        }],
        dropped_lines: 0,
        source_len: text.len(),
        rendered_text: None,
        links: Vec::new(),
    }
}

/// A leaf that emits text runs the way `TextWidget` does.
#[derive(Debug)]
struct Reviewable {
    text: String,
    role: Role,
    /// When false the runs are emitted without any measured geometry, the
    /// state a label painted with no text backend is in.
    measured: bool,
}

impl Reviewable {
    fn new(text: &str) -> Self {
        Self {
            text: text.to_string(),
            role: Role::Label,
            measured: true,
        }
    }
    fn with_role(mut self, role: Role) -> Self {
        self.role = role;
        self
    }
    fn unmeasured(mut self) -> Self {
        self.measured = false;
        self
    }
}

impl Widget for Reviewable {
    fn layout_response(&self, _proposal: SizeProposal, _ctx: &LayoutContext) -> LayoutResponse {
        Size::new(self.text.chars().count() as f32 * CHAR_WIDTH, LINE_HEIGHT).into()
    }

    fn accessibility(&self, builder: &mut AccessNodeBuilder) {
        builder.set_role(self.role);
        let geometry = measured(&self.text);
        let source = if self.measured {
            TextRunSource::from_geometry(&self.text, &geometry, teksilo_canvas::Point::ZERO, 0)
        } else {
            TextRunSource::flat(&self.text, 0).with_fallback_rect(Rect::new(
                0.0,
                0.0,
                self.text.chars().count() as f32 * CHAR_WIDTH,
                LINE_HEIGHT,
            ))
        };
        let emission = push_text_runs(builder, None, &source);
        builder.set_name(&emission.value);
    }
}

fn tree_with(widget: Reviewable) -> (TreeUpdate, NodeId) {
    let mut tree = WidgetTree::new();
    let id = tree.add(widget);
    tree.layout(SizeProposal::exact(400.0, 100.0));
    let update = tree.sync_accessibility();
    (
        update,
        teksilo_core::accessibility::widget_id_to_node_id(id),
    )
}

/// Locate a node in the consumer's tree by the local id it reports.
fn find<'a>(state: &'a accesskit_consumer::TreeState, id: NodeId) -> NodeRef<'a> {
    let mut stack = vec![state.root()];
    while let Some(node) = stack.pop() {
        if node.locate().0 == id {
            return node;
        }
        for child in node.children() {
            stack.push(child);
        }
    }
    panic!("node {id:?} is absent from the emitted tree");
}

#[test]
fn a_label_with_runs_supports_text_ranges() {
    let (update, id) = tree_with(Reviewable::new("Hello world"));
    let consumer = Tree::new(update, false);
    let state = consumer.state();
    assert!(
        find(state, id).supports_text_ranges(),
        "a label with text-run children must be reviewable by character, \
         word and line — `supports_text_ranges` is what every platform's \
         text API is gated on"
    );
}

#[test]
fn the_document_text_equals_the_source() {
    // The consumer derives a node's document text from its runs while
    // every platform announces the node's own value. A divergence is a
    // place where what a reader hears and what it reviews differ.
    let (update, id) = tree_with(Reviewable::new("Hello world"));
    let consumer = Tree::new(update, false);
    let state = consumer.state();
    let node = find(state, id);
    assert_eq!(node.document_range().text(), "Hello world");
    assert_eq!(node.value(), Some("Hello world".to_string()));
}

#[test]
fn every_range_reports_a_bounding_box() {
    // `Range::bounding_boxes` throws away every box it has collected the
    // moment one run lacks bounds, direction, positions or widths — so a
    // single half-populated run empties the geometry of the whole label
    // and a magnifier stops following the review cursor entirely.
    let (update, id) = tree_with(Reviewable::new("Hello world"));
    let consumer = Tree::new(update, false);
    let state = consumer.state();
    let node = find(state, id);

    let whole = node.document_range();
    assert!(
        !whole.bounding_boxes().is_empty(),
        "the whole label reports no geometry"
    );

    // Every single-character range, walked the way a review cursor walks.
    //
    // The walk stops on `is_document_end` rather than on a position that
    // stops moving: `forward_to_character_start` adds one unconditionally
    // (`accesskit_consumer-0.39.0/src/text.rs:308`), so stepping past the
    // last character yields an index one past the run and `bounding_boxes`
    // then indexes out of bounds. That is the consumer's contract, not a
    // defect here — a caller must not step past the end.
    let mut position = node.document_start();
    let mut checked = 0usize;
    while !position.is_document_end() {
        let next = position.forward_to_character_start();
        let mut range = position.to_degenerate_range();
        range.set_end(next);
        assert!(
            !range.bounding_boxes().is_empty(),
            "character {checked} reports no bounding box"
        );
        position = next;
        checked += 1;
    }
    assert_eq!(checked, "Hello world".chars().count());

    // And the word ranges, which is what braille routing asks for.
    let word = node.document_start().forward_to_word_start();
    let mut range = node.document_start().to_degenerate_range();
    range.set_end(word);
    assert!(
        !range.bounding_boxes().is_empty(),
        "the first word has no box"
    );
}

#[test]
fn an_unmeasured_label_still_reports_a_box_for_every_range() {
    // Degenerate geometry is the whole point of the universal rule: a
    // label painted with no measuring backend keeps answering
    // `AXBoundsForRange`, with a zero-width rectangle at its leading edge,
    // rather than reporting nothing and taking the rest of the label's
    // geometry down with it.
    let (update, id) = tree_with(Reviewable::new("Hello").unmeasured());
    let consumer = Tree::new(update, false);
    let state = consumer.state();
    let node = find(state, id);
    assert!(node.supports_text_ranges());
    assert!(!node.document_range().bounding_boxes().is_empty());
}

#[test]
fn a_non_text_role_supports_no_ranges() {
    // Runs under a role no platform reads them for are inert, so the
    // builder drops them. The node must then report no ranges at all
    // rather than ranges nobody can reach.
    let (update, id) = tree_with(Reviewable::new("Save").with_role(Role::Button));
    let consumer = Tree::new(update, false);
    let state = consumer.state();
    let node = find(state, id);
    assert!(!node.supports_text_ranges());
    assert_eq!(node.children().count(), 0, "the inert runs must be gone");
}
