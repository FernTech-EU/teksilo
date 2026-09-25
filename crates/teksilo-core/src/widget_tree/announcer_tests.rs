// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! The framework's announcer, as a platform adapter receives it.
//!
//! Every update is replayed through `accesskit_consumer`, the crate all three
//! adapters are built on, with the rule `accesskit_atspi_common` uses to decide
//! that a node entered or left the tree a reader sees (`adapter.rs:280-349`):
//! a node that starts to pass `common_filter` is added, and one that stops is
//! removed, which AT-SPI announces as defunct. What these tests hold the
//! announcer to is what `tools/reader/` measured Orca 46.1 needing (the
//! `announcer-calendar-arrows` scenario):
//!
//! - a message spoken from a node the adapter had once removed came from an
//!   object Orca's AT-SPI library still held for dead, and was dropped;
//! - a message whose node was gone by the time Orca asked it for its role was
//!   dropped too, and Orca asked tens of milliseconds after the event, which
//!   was already after a node that lived one update.

use std::collections::HashSet;
use std::time::Duration;

use accesskit::NodeId;
use accesskit_consumer::{FilterResult, NodeRef, Tree, TreeChangeHandler, common_filter};
use teksilo_canvas::SizeProposal;

use crate::announcer::Politeness;
use crate::test_widgets::FillWidget;
use crate::widget_tree::WidgetTree;

/// A tree with one widget in it, laid out and synced once.
fn settled() -> WidgetTree {
    let mut tree = WidgetTree::new();
    tree.add(FillWidget::new());
    tree.layout(SizeProposal::exact(400.0, 300.0));
    tree
}

/// A platform adapter's view of the tree: which nodes it has created, which it
/// has removed, and which live nodes it announced as they arrived.
struct Adapter {
    replay: Tree,
    /// Every node the adapter has ever removed, and so announced as defunct.
    defunct: HashSet<NodeId>,
}

#[derive(Default)]
struct Changes {
    /// Live nodes that entered, with their name, in no particular order.
    announced: Vec<(NodeId, String)>,
    removed: Vec<NodeId>,
}

fn included(node: &NodeRef) -> bool {
    common_filter(node) == FilterResult::Include
}

impl Changes {
    fn entered(&mut self, node: &NodeRef) {
        if node.live() != accesskit::Live::Off
            && let Some(name) = node.label()
        {
            self.announced.push((node.locate().0, name));
        }
    }
}

impl TreeChangeHandler for Changes {
    fn node_added(&mut self, node: &NodeRef) {
        if included(node) {
            self.entered(node);
        }
    }

    fn node_updated(&mut self, old: &NodeRef, new: &NodeRef) {
        match (included(old), included(new)) {
            (false, true) => self.entered(new),
            (true, false) => self.removed.push(old.locate().0),
            _ => {}
        }
    }

    fn focus_moved(&mut self, _old: Option<&NodeRef>, _new: Option<&NodeRef>) {}

    fn node_removed(&mut self, node: &NodeRef) {
        if included(node) {
            self.removed.push(node.locate().0);
        }
    }
}

impl Adapter {
    fn attach(tree: &mut WidgetTree) -> Self {
        Self {
            replay: Tree::new(tree.sync_accessibility(), true),
            defunct: HashSet::new(),
        }
    }

    /// Run one frame, as `teksilo-app` does (lay out, then sync), and report
    /// what the adapter did with the update.
    fn sync(&mut self, tree: &mut WidgetTree) -> Changes {
        tree.layout(SizeProposal::exact(400.0, 300.0));
        let mut changes = Changes::default();
        self.replay
            .update_and_process_changes(tree.sync_accessibility(), &mut changes);
        changes
    }

    /// How many announcer nodes the adapter has in the tree now.
    fn announcer_nodes(&self) -> usize {
        self.replay
            .state()
            .root()
            .children()
            .filter(|child| {
                crate::announcer::is_announcer_node(child.locate().0) && included(child)
            })
            .count()
    }

    /// Whether a live node named `text` is in the tree a reader walks now.
    fn holds(&self, text: &str) -> bool {
        fn walk(node: NodeRef<'_>, text: &str) -> bool {
            (node.live() != accesskit::Live::Off && node.label().as_deref() == Some(text))
                || node
                    .filtered_children(&common_filter)
                    .any(|child| walk(child, text))
        }
        walk(self.replay.state().root(), text)
    }
}

/// The defect `tools/reader/` found in every Teksilo application: after the
/// first message of a session, every message came from a node the adapter
/// had already removed, and Orca dropped it as defunct.
#[test]
fn every_message_comes_from_a_node_the_platform_has_never_had() {
    let mut tree = settled();
    let mut adapter = Adapter::attach(&mut tree);
    // The same text twice on purpose: a repeat is the case the announcer
    // exists for, and it must still be a new node.
    for message in ["Saved", "Saved", "Deleted", "Saved"] {
        let removed_before = adapter.defunct.clone();
        tree.announce(message);
        let mut spoken = Vec::new();
        // Enough frames for any cycle of showing and taking away, then enough
        // time for any node the message was spoken from to have left.
        for _ in 0..4 {
            let changes = adapter.sync(&mut tree);
            spoken.extend(changes.announced);
            adapter.defunct.extend(changes.removed);
        }
        tree.advance_time(Duration::from_secs(10));
        for _ in 0..2 {
            let changes = adapter.sync(&mut tree);
            spoken.extend(changes.announced);
            adapter.defunct.extend(changes.removed);
        }
        let from: Vec<NodeId> = spoken
            .iter()
            .filter(|(_, text)| text == message)
            .map(|(id, _)| *id)
            .collect();
        assert_eq!(
            from.len(),
            1,
            "{message:?} must be announced once: {spoken:?}"
        );
        assert!(
            !removed_before.contains(&from[0]),
            "{message:?} was spoken from {:?}, a node the adapter had already removed \
             and announced as defunct; Orca drops what it says",
            from[0]
        );
        // Its own node is removed now, and never to be used again.
        assert!(
            adapter.defunct.contains(&from[0]),
            "{message:?}'s node must have left the tree once its time was over"
        );
    }
}

/// The other half of what Orca needed: a message's node must still be there
/// when a reader asks it for its role, which is after the event, not at it.
#[test]
fn a_message_stays_in_the_tree_until_a_reader_can_ask_for_it() {
    let mut tree = settled();
    let mut adapter = Adapter::attach(&mut tree);
    tree.advance_time(Duration::ZERO);
    tree.announce("Event added");
    let first = adapter.sync(&mut tree);
    assert!(
        first
            .announced
            .iter()
            .any(|(_, text)| text == "Event added"),
        "the message must be announced on the first update after it is asked for"
    );
    // The next frames, a few milliseconds later each.
    for frame in 1..=3 {
        tree.advance_time(Duration::from_millis(16));
        let _ = adapter.sync(&mut tree);
        assert!(
            adapter.holds("Event added"),
            "frame {frame}: the message's node left the tree before a reader could \
             ask for it; Orca answers \"Unknown object\" and drops the announcement"
        );
    }
    // A second later, far past anything a reader took to ask.
    tree.advance_time(Duration::from_secs(1));
    let _ = adapter.sync(&mut tree);
    assert!(
        adapter.holds("Event added"),
        "a second on, the node must still be there"
    );
    // And long after, it has gone: nobody walking the window later meets it.
    tree.advance_time(Duration::from_secs(10));
    let _ = adapter.sync(&mut tree);
    assert!(
        !adapter.holds("Event added"),
        "ten seconds on, the message must have left the tree"
    );
}

/// Two messages asked for together are spoken in the order they were asked
/// for, and the first is still there to be read when the second arrives.
#[test]
fn a_second_message_does_not_take_the_first_away() {
    let mut tree = settled();
    let mut adapter = Adapter::attach(&mut tree);
    tree.advance_time(Duration::ZERO);
    tree.announce("Moved to position 3");
    tree.announce("Moved to position 4");
    let mut order = Vec::new();
    for _ in 0..4 {
        let changes = adapter.sync(&mut tree);
        order.extend(changes.announced.into_iter().map(|(_, text)| text));
        if order.len() == 2 {
            break;
        }
        tree.advance_time(Duration::from_millis(16));
    }
    assert_eq!(order, ["Moved to position 3", "Moved to position 4"]);
    assert!(
        adapter.holds("Moved to position 3"),
        "the first message left the tree as the second arrived, before a reader \
         busy with the second could ask the first for its role"
    );
}

/// A spoken message leaves the tree by itself: the tree asks the event loop to
/// wake when it is due to go, and does not spin frames until then.
#[test]
fn a_spoken_message_wakes_the_loop_once_to_leave() {
    let mut tree = settled();
    let mut adapter = Adapter::attach(&mut tree);
    tree.announce_with("Upload failed", Politeness::Assertive);
    let _ = adapter.sync(&mut tree);
    let _ = adapter.sync(&mut tree);
    let now = std::time::Instant::now();
    let deadline = tree
        .next_timer_deadline()
        .expect("a message in the tree must ask the loop to wake when it leaves");
    assert!(
        deadline > now + Duration::from_secs(1),
        "the loop is woken when the message leaves, not every frame until then \
         (deadline {:?} from now)",
        deadline.saturating_duration_since(now)
    );
}

/// A burst of messages does not grow the tree without bound.
#[test]
fn a_burst_of_messages_keeps_the_tree_small() {
    let mut tree = settled();
    let mut adapter = Adapter::attach(&mut tree);
    tree.advance_time(Duration::ZERO);
    for i in 0..30 {
        tree.announce(format!("Row {i}"));
    }
    let mut heard = Vec::new();
    for _ in 0..80 {
        let changes = adapter.sync(&mut tree);
        heard.extend(changes.announced.into_iter().map(|(_, text)| text));
        let live = adapter.announcer_nodes();
        assert!(live <= 8, "{live} announcer nodes in the tree at once");
        tree.advance_time(Duration::from_millis(16));
    }
    let expected: Vec<String> = (0..30).map(|i| format!("Row {i}")).collect();
    assert_eq!(heard, expected, "every message is heard, in order");
}
