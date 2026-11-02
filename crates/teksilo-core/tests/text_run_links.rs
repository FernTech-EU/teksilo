// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Links inside laid-out text, as `push_text_runs_with_links` emits them and
//! `accesskit_consumer` (the crate every platform adapter answers through)
//! reads them: a `Role::Link` in reading order, one character space across
//! the links, one visual line however many nodes its runs hang from.

use std::ops::Range;

use accesskit::{Action, NodeId, Role, TreeUpdate};
use accesskit_consumer::{NodeRef, Tree};
use teksilo_canvas::{
    CharGeom, LineEnd, Point, Size, SizeProposal, TextDirection, TextGeometry, TextLine,
    TextLineSegment,
};
use teksilo_core::accessibility::AccessNodeBuilder;
use teksilo_core::accessibility::text_runs::{LinkSpan, TextRunSource, push_text_runs_with_links};
use teksilo_core::widget::{LayoutContext, LayoutResponse, Widget};
use teksilo_core::widget_tree::WidgetTree;

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
    let byte_range: Range<usize> = 0..text.len();
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

/// Lines starting at `starts` (byte offsets), each one segment in
/// `direction`, the last ending in a hard break when `hard_break` (its
/// trailing `\n` then unmeasured, as a layout reports it).
fn lines(text: &str, starts: &[usize], direction: TextDirection, hard_break: bool) -> TextGeometry {
    let mut geometry = measured(text);
    geometry.lines.clear();
    let mut char_start = 0;
    for (index, &start) in starts.iter().enumerate() {
        let end = starts.get(index + 1).copied().unwrap_or(text.len());
        let last = index + 1 == starts.len();
        let body_end = if last && hard_break { end - 1 } else { end };
        let body = &text[start..body_end];
        let count = body.chars().count();
        let characters: Vec<CharGeom> = (0..count)
            .map(|i| CharGeom {
                position: i as f32 * CHAR_WIDTH,
                width: CHAR_WIDTH,
            })
            .collect();
        let width = count as f32 * CHAR_WIDTH;
        let y = index as f32 * LINE_HEIGHT;
        let line_chars = text[start..end].chars().count();
        geometry.lines.push(TextLine {
            index,
            byte_range: start..end,
            char_range: char_start..char_start + line_chars,
            rect: [0.0, y, width, LINE_HEIGHT],
            baseline: y + LINE_HEIGHT * 0.75,
            caret_x: 0.0,
            segments: vec![TextLineSegment {
                byte_range: start..body_end,
                char_range: char_start..char_start + count,
                direction,
                rect: [0.0, y, width, LINE_HEIGHT],
                characters,
            }],
            end: if last && hard_break {
                LineEnd::HardBreak { chars: 1, bytes: 1 }
            } else if last {
                LineEnd::EndOfText
            } else {
                LineEnd::SoftWrap
            },
            truncation: None,
        });
        char_start += line_chars;
    }
    geometry
}

/// A label whose text holds links at the given byte ranges.
#[derive(Debug)]
struct Linked {
    text: String,
    links: Vec<(Range<usize>, &'static str)>,
    geometry: Option<TextGeometry>,
}

impl Widget for Linked {
    fn layout_response(&self, _proposal: SizeProposal, _ctx: &LayoutContext) -> LayoutResponse {
        Size::new(self.text.chars().count() as f32 * CHAR_WIDTH, LINE_HEIGHT).into()
    }

    fn accessibility(&self, builder: &mut AccessNodeBuilder) {
        builder.set_role(Role::Label);
        let geometry = self
            .geometry
            .clone()
            .unwrap_or_else(|| measured(&self.text));
        let source = TextRunSource::from_geometry(&self.text, &geometry, Point::ZERO, 0);
        let spans: Vec<LinkSpan> = self
            .links
            .iter()
            .enumerate()
            .map(|(i, (range, url))| LinkSpan {
                byte_range: range.clone(),
                element_id: i as u64 + 1,
                label: self.text[range.clone()].to_string(),
                url: url.to_string(),
            })
            .collect();
        let emission = push_text_runs_with_links(builder, None, &source, &spans);
        // The emitter advertises no action; a widget that follows links does.
        for link in &emission.links {
            builder.with_collected_node(link.id, |node| node.add_action(Action::Click));
        }
        builder.set_name(&emission.runs.value);
    }
}

fn tree_with(widget: Linked) -> (TreeUpdate, NodeId) {
    let mut tree = WidgetTree::new();
    let id = tree.add(widget);
    tree.layout(SizeProposal::exact(400.0, 100.0));
    let update = tree.sync_accessibility();
    (
        update,
        teksilo_core::accessibility::widget_id_to_node_id(id),
    )
}

fn find<'a>(state: &'a accesskit_consumer::TreeState, id: NodeId) -> NodeRef<'a> {
    let mut stack = vec![state.root()];
    while let Some(node) = stack.pop() {
        if node.locate().0 == id {
            return node;
        }
        stack.extend(node.children());
    }
    panic!("node {id:?} is absent from the emitted tree");
}

fn see_backups() -> Linked {
    Linked {
        text: "see backups for more".into(),
        links: vec![(4..11, "help:backups")],
        geometry: None,
    }
}

/// The link is a `Role::Link` named by its text, with its destination as
/// value and Click advertised, and it sits between the text before it and
/// the text after it.
#[test]
fn a_link_is_a_link_node_in_reading_order() {
    let (update, id) = tree_with(see_backups());
    let consumer = Tree::new(update, false);
    let owner = find(consumer.state(), id);
    let children: Vec<NodeRef<'_>> = owner.children().collect();
    let roles: Vec<Role> = children.iter().map(|c| c.role()).collect();
    assert_eq!(roles, [Role::TextRun, Role::Link, Role::TextRun]);
    let link = &children[1];
    assert_eq!(link.label().as_deref(), Some("backups"));
    assert_eq!(link.value().as_deref(), Some("help:backups"));
    assert!(link.data().supports_action(Action::Click));
}

/// The link's runs hang from a text container that supports text ranges,
/// so their text changes reach the platform, and the owner's text is the
/// whole source, in one character space.
#[test]
fn a_links_runs_hang_from_a_text_container_and_keep_one_character_space() {
    let (update, id) = tree_with(see_backups());
    let consumer = Tree::new(update, false);
    let owner = find(consumer.state(), id);
    let link = owner
        .children()
        .find(|c| c.role() == Role::Link)
        .expect("a link node");
    let container = link.children().next().expect("a text container");
    assert_eq!(container.role(), Role::Label);
    assert!(container.supports_text_ranges());
    assert_eq!(container.document_range().text(), "backups");
    assert_eq!(owner.document_range().text(), "see backups for more");
}

/// Every run of the line is on one line for a reader moving by line: the
/// `next_on_line` chain runs from the first run to the last through the
/// link's run.
#[test]
fn the_line_stays_one_line_across_the_link() {
    let (update, id) = tree_with(see_backups());
    let consumer = Tree::new(update, false);
    let owner = find(consumer.state(), id);
    let first = owner.children().next().expect("a first run");
    let mut texts = vec![first.value().unwrap_or_default()];
    let mut next = first.data().next_on_line();
    while let Some(id) = next {
        let node = find(consumer.state(), id);
        texts.push(node.value().unwrap_or_default());
        next = node.data().next_on_line();
    }
    assert_eq!(texts, ["see ", "backups", " for more"]);
}

/// The link node has a box, the union of its runs' boxes.
#[test]
fn a_link_has_the_box_of_its_text() {
    let (update, id) = tree_with(see_backups());
    let consumer = Tree::new(update, false);
    let owner = find(consumer.state(), id);
    let link = owner
        .children()
        .find(|c| c.role() == Role::Link)
        .expect("a link node");
    let link_box = link.bounding_box().expect("the link has a box");
    let run_box = link
        .children()
        .next()
        .and_then(|c| c.children().next())
        .and_then(|r| r.bounding_box())
        .expect("its run has a box");
    assert!(
        link_box.x0 <= run_box.x0 + 0.01
            && link_box.x1 >= run_box.x1 - 0.01
            && link_box.y0 <= run_box.y0 + 0.01
            && link_box.y1 >= run_box.y1 - 0.01,
        "link {link_box:?} must hold its run {run_box:?}"
    );
    assert!((link_box.x1 - link_box.x0 - 7.0 * CHAR_WIDTH as f64).abs() < 0.01);
}

/// Two links that touch are two links, one after the other.
#[test]
fn touching_links_stay_two_links() {
    let (update, id) = tree_with(Linked {
        text: "onetwo".into(),
        links: vec![(0..3, "a:one"), (3..6, "a:two")],
        geometry: None,
    });
    let consumer = Tree::new(update, false);
    let owner = find(consumer.state(), id);
    let names: Vec<String> = owner
        .children()
        .filter(|c| c.role() == Role::Link)
        .map(|c| c.label().unwrap_or_default())
        .collect();
    assert_eq!(names, ["one", "two"]);
    assert_eq!(owner.document_range().text(), "onetwo");
}

/// The runs a node holds, through its link's container, with their boxes.
fn runs_under<'a>(node: &NodeRef<'a>) -> Vec<NodeRef<'a>> {
    let mut out = Vec::new();
    for child in node.children() {
        if child.role() == Role::TextRun {
            out.push(child);
        } else {
            out.extend(runs_under(&child));
        }
    }
    out
}

/// A link that wraps: one link and one container hold both lines' runs, each
/// run keeps its own line's box and its line's chain, and the link's box is
/// the union of the two.
#[test]
fn a_wrapped_link_is_one_link_over_two_lines() {
    let text = "see backups for more";
    let (update, id) = tree_with(Linked {
        text: text.into(),
        links: vec![(4..11, "help:backups")],
        geometry: Some(lines(text, &[0, 8], TextDirection::LeftToRight, false)),
    });
    let consumer = Tree::new(update, false);
    let owner = find(consumer.state(), id);
    let links: Vec<NodeRef<'_>> = owner
        .children()
        .filter(|c| c.role() == Role::Link)
        .collect();
    assert_eq!(links.len(), 1, "one link");
    let runs = runs_under(&links[0]);
    let texts: Vec<String> = runs.iter().map(|r| r.value().unwrap_or_default()).collect();
    assert_eq!(texts, ["back", "ups"]);
    let first = runs[0].bounding_box().expect("a box");
    let second = runs[1].bounding_box().expect("a box");
    assert!(
        second.y0 >= first.y1 - 0.01,
        "each run on its own line: {first:?} {second:?}"
    );
    let link_box = links[0].bounding_box().expect("the link has a box");
    assert!(link_box.y0 <= first.y0 + 0.01 && link_box.y1 >= second.y1 - 0.01);
    // Each line's chain: "see " to "back", then "ups" to " for more".
    assert_eq!(
        runs[0]
            .data()
            .previous_on_line()
            .map(|p| find(consumer.state(), p).value()),
        Some(Some("see ".to_string()))
    );
    assert_eq!(runs[0].data().next_on_line(), None, "back ends its line");
    assert_eq!(
        runs[1].data().previous_on_line(),
        None,
        "ups starts the next"
    );
    assert_eq!(owner.document_range().text(), text);
}

/// A link in right-to-left text keeps its run's box and its line's chain.
#[test]
fn a_right_to_left_link_holds_its_runs_box() {
    let text = "abc def ghi";
    let (update, id) = tree_with(Linked {
        text: text.into(),
        links: vec![(4..7, "a:def")],
        geometry: Some(lines(text, &[0], TextDirection::RightToLeft, false)),
    });
    let consumer = Tree::new(update, false);
    let owner = find(consumer.state(), id);
    let link = owner
        .children()
        .find(|c| c.role() == Role::Link)
        .expect("a link node");
    let run = runs_under(&link).pop().expect("its run");
    assert_eq!(run.value().as_deref(), Some("def"));
    let run_box = run.bounding_box().expect("a box");
    let link_box = link.bounding_box().expect("the link has a box");
    assert!((link_box.x0 - run_box.x0).abs() < 0.01 && (link_box.x1 - run_box.x1).abs() < 0.01);
    assert!(run.data().previous_on_line().is_some() && run.data().next_on_line().is_some());
    assert_eq!(owner.document_range().text(), text);
}

/// A line whose text ends in a link puts its hard break on the link's last
/// run: the break is never a run of its own beside text.
#[test]
fn a_break_after_a_link_rides_the_links_run() {
    let text = "see backups\n";
    let (update, id) = tree_with(Linked {
        text: text.into(),
        links: vec![(4..11, "help:backups")],
        geometry: Some(lines(text, &[0], TextDirection::LeftToRight, true)),
    });
    let consumer = Tree::new(update, false);
    let owner = find(consumer.state(), id);
    let link = owner
        .children()
        .find(|c| c.role() == Role::Link)
        .expect("a link node");
    let runs = runs_under(&link);
    assert_eq!(runs.len(), 1);
    assert_eq!(runs[0].value().as_deref(), Some("backups\n"));
    assert_eq!(owner.document_range().text(), text);
}
