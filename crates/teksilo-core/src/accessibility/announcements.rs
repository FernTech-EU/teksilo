// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! What the platform accessibility layers announce, replayed in process.
//!
//! Nothing in process can hear what a screen reader said, and a headless tree
//! has no platform layer at all. The automation bridge's `pull_announcements`,
//! and every test that asks whether something was spoken, read the ring this
//! module fills instead. So the ring must hold what the platform adapters
//! would have announced, and nothing else: an entry the adapters would not
//! have produced is a probe passing over silence.
//!
//! That is why the ring does not read the tree it is given. Every update is
//! replayed into an `accesskit_consumer::Tree`, the diff all three adapters
//! are built on, and the adapters' own live-region rules are applied to the
//! changes it reports. Read out of `accesskit_atspi_common-0.20.0`,
//! `accesskit_windows-0.35.0` and `accesskit_macos-0.27.0`:
//!
//! * **What is said** is the node's name as the consumer computes it: the
//!   `value` of a `Role::Label` (`label_comes_from_value`, consumer
//!   `node.rs:744-746`), the label of any other role, followed through
//!   `labelled_by` and, for a button-like role, through its descendant labels
//!   (AT-SPI `node.rs:38-44`, Windows `node.rs:404-413`, macOS
//!   `node.rs:328-334`). A live `Status` or `Alert` that carries its text only
//!   as a `value` has no name, and all three stay silent about it.
//! * **How urgently** is the consumer's `live()`, which a node without a
//!   setting of its own inherits from its nearest ancestor that has one
//!   (consumer `node.rs:906-910`). Everything inside a live region is live.
//! * **When** is one of two moments. A live, named node that enters the
//!   filtered tree, whether it was just added or was excluded until now, is
//!   announced by all three (AT-SPI `adapter.rs:72-77`, reached from
//!   `node_added` at 281-285 and `node_updated` at 291-297; Windows
//!   `adapter.rs:256-263` and 313-324; macOS `event.rs:236-241` and
//!   300-310). A live node whose name changes while it stays in the filtered
//!   tree is announced by all three (AT-SPI `node.rs:610-622`, and the same
//!   Windows and macOS lines).
//! * **Never** for a node `common_filter` excludes: a hidden node, anything
//!   inside a hidden subtree, a node its clipping parent has scrolled out of
//!   view, a `GenericContainer`, a text run. A focused node is never excluded.
//!
//! Two announcements happen on some platforms only, and the ring records
//! neither, because the one reading it may be running either screen reader and
//! must not be told something was said that one of them kept quiet about:
//!
//! * a live node whose politeness alone changes is announced by Windows and
//!   macOS, and not by AT-SPI;
//! * when a subtree that was excluded as a whole comes back, AT-SPI announces
//!   every live, named node inside it (`add_subtree`, `adapter.rs:84-89`),
//!   while Windows and macOS announce only the nodes the consumer reports,
//!   whose own data changed.
//!
//! A blank name is not recorded either. AT-SPI sends an empty announcement
//! when a live node's name is cleared, and there is nothing in it to speak.
//!
//! The replay starts where an adapter starts when a screen reader is already
//! running as a window opens: from a bare window node, the tree
//! `teksilo-platform` hands an adapter that asks before the window has drawn.
//! So whatever is live and named in the first update is announced, as a node
//! arriving. The window is taken to have focus, as it has while somebody is
//! using it; that decides only whether the focused node is exempt from the
//! filter.
//!
//! # Who pays for the replay
//!
//! Replaying is a pass of the consumer over every node of every update, which
//! makes a full sync of 3,000 nodes 1.4 to 2 times as long (the more of them
//! change, the more), and it holds a second copy of the tree. Only a reader
//! gains anything by it, and the ring has two: the automation bridge, and
//! tests, the framework's and an application's own, which read
//! [`WidgetTree::announcements_since`] from a tree they build headlessly. So
//! recording is a property of the tree
//! ([`WidgetTree::set_records_announcements`]). A tree built with
//! `WidgetTree::new()` records, which is every headless tree and every test;
//! `teksilo-app` switches it off on the tree of each window it opens, unless
//! the automation bridge was installed, and a released application pays
//! nothing. Nor can a tree that is not recording abort, where panics abort,
//! on an update the consumer rejects, since it hands the ring's consumer
//! none; [`speaks_within`] still builds one, once per gesture that asks.
//!
//! Recording can be switched on at any time. The updates handed out while it
//! was off have already reached whoever was listening, so the first update
//! after it is switched on seeds a new replay without announcing anything, and
//! the changes after that are heard.
//!
//! [`WidgetTree::announcements_since`]: crate::WidgetTree::announcements_since
//! [`WidgetTree::set_records_announcements`]: crate::WidgetTree::set_records_announcements

use std::collections::VecDeque;

use accesskit::{Live, NodeId, TreeUpdate};
use accesskit_consumer::{FilterResult, NodeRef, Tree, TreeChangeHandler, common_filter};

use super::Announcement;

/// How many announcements the ring keeps. The oldest is dropped when it is
/// full, so a reader that lags far behind sees only the most recent ones.
pub(crate) const CAPACITY: usize = 256;

/// The announcements every platform adapter would have made, in the order the
/// updates carried them. See the [module documentation](self).
pub(crate) struct AnnouncementRing {
    /// Whether updates are replayed and recorded at all. See the
    /// [module documentation](self#who-pays-for-the-replay).
    recording: bool,
    /// The adapters' view of the tree, as of the last update observed. `None`
    /// before the first update, after one the consumer could not apply, and
    /// after one handed out while not recording.
    replay: Option<Tree>,
    /// Set after an update the consumer could not apply, or one handed out
    /// while not recording. The replay is gone with it, and starting over from
    /// a bare window would announce every live node in the application as
    /// though it had just appeared, so the next update seeds a new replay
    /// without announcing anything.
    reseed_quietly: bool,
    entries: VecDeque<Announcement>,
    /// The sequence number of the last entry recorded; the first is 1.
    last_seq: u64,
}

impl AnnouncementRing {
    pub(crate) fn new() -> Self {
        Self {
            recording: true,
            replay: None,
            reseed_quietly: false,
            entries: VecDeque::new(),
            last_seq: 0,
        }
    }

    /// Replay `update` as the adapters would receive it, and record what they
    /// would announce.
    ///
    /// The consumer panics on an update it cannot apply, exactly as it would
    /// inside a platform adapter. Here that would bring down an application no
    /// screen reader is even attached to, over a record kept for automation,
    /// so the panic is caught and the replay dropped. The panic hook has
    /// already reported it by then. Where panics abort, nothing is caught.
    pub(crate) fn observe(&mut self, update: &TreeUpdate) {
        if !self.recording {
            // Whatever this update announces is announced now, unrecorded,
            // and the replay no longer matches what the adapters hold.
            self.replay = None;
            self.reseed_quietly = true;
            return;
        }
        let replay = self.replay.take();
        if replay.is_none() && update.tree.is_none() {
            // Only an update that carries tree data can start a tree; this one
            // could only be applied to a replay that no longer exists.
            return;
        }
        let quietly = self.reseed_quietly;
        let applied = std::panic::catch_unwind(std::panic::AssertUnwindSafe(move || {
            let mut heard = Heard::default();
            let tree = match replay {
                Some(mut tree) => {
                    tree.update_and_process_changes(update.clone(), &mut heard);
                    tree
                }
                None if quietly => Tree::new(update.clone(), true),
                None => {
                    let mut tree = Tree::new(before_first_draw(), true);
                    tree.update_and_process_changes(update.clone(), &mut heard);
                    tree
                }
            };
            let spoken = heard.in_reading_order(&tree);
            (tree, spoken)
        }));
        match applied {
            Ok((tree, spoken)) => {
                self.replay = Some(tree);
                self.reseed_quietly = false;
                for (text, assertive) in spoken {
                    self.record(text, assertive);
                }
            }
            Err(_) => self.reseed_quietly = true,
        }
    }

    fn record(&mut self, text: String, assertive: bool) {
        self.last_seq = self.last_seq.saturating_add(1);
        if self.entries.len() >= CAPACITY {
            self.entries.pop_front();
        }
        self.entries.push_back(Announcement {
            seq: self.last_seq,
            text,
            assertive,
        });
    }

    /// Whether updates are replayed and recorded.
    pub(crate) fn is_recording(&self) -> bool {
        self.recording
    }

    /// Start or stop replaying and recording. The entries already recorded
    /// stay readable either way.
    pub(crate) fn set_recording(&mut self, recording: bool) {
        self.recording = recording;
    }

    /// Whether a replay is held, which is the memory and the work recording
    /// costs.
    #[cfg(test)]
    pub(crate) fn holds_a_replay(&self) -> bool {
        self.replay.is_some()
    }

    /// Every kept announcement whose `seq` is greater than `seq`.
    pub(crate) fn since(&self, seq: u64) -> Vec<Announcement> {
        self.entries
            .iter()
            .filter(|announcement| announcement.seq > seq)
            .cloned()
            .collect()
    }
}

/// The tree an adapter holds when a screen reader asked for one before the
/// window first drew: a window node and nothing under it. The same tree as
/// `teksilo-platform`'s `empty_initial_tree`, which this crate cannot name.
fn before_first_draw() -> TreeUpdate {
    let root = super::root_node_id();
    TreeUpdate {
        nodes: vec![(root, accesskit::Node::new(accesskit::Role::Window))],
        tree: Some(accesskit::TreeInfo::new(root)),
        tree_id: accesskit::TreeId::ROOT,
        focus: root,
    }
}

/// Whether `update` holds a node `owned` accepts that an adapter would speak
/// for: in the filtered tree, live, with a name that is not blank.
///
/// The politeness has to come from an owned node as well, the node itself or
/// the owned ancestor it inherits from. A button inside somebody else's live
/// region is live to the adapters, but that is the region speaking, and it
/// says nothing about what the button just did.
///
/// Read through a consumer tree built from `update` on the spot, not through
/// the ring's replay, so it answers the same whether or not the tree records
/// announcements. It is asked once per gesture, not once per frame. A
/// consumer that rejects the update is taken to mean nothing speaks, so the
/// framework's own message is said rather than lost.
pub(crate) fn speaks_within(update: &TreeUpdate, owned: impl Fn(NodeId) -> bool) -> bool {
    let built = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        Tree::new(update.clone(), true)
    }));
    let Ok(tree) = built else {
        return false;
    };
    let state = tree.state();
    let mut stack = vec![state.root()];
    while let Some(node) = stack.pop() {
        if owned(node.locate().0)
            && common_filter(&node) == FilterResult::Include
            && node.live() != Live::Off
            && politeness_source(node).is_some_and(|source| owned(source.locate().0))
            && spoken_name(&node).is_some_and(|name| !name.trim().is_empty())
        {
            return true;
        }
        stack.extend(node.children());
    }
    false
}

/// The node `node` takes its politeness from: itself, or the nearest ancestor
/// that sets one, which is how the consumer's `live()` resolves it. `None`
/// when nothing on the way to the root sets one.
fn politeness_source(node: NodeRef<'_>) -> Option<NodeRef<'_>> {
    let mut current = Some(node);
    while let Some(candidate) = current {
        if candidate.data().live().is_some() {
            return Some(candidate);
        }
        current = candidate.parent();
    }
    None
}

/// The name every adapter speaks for `node`. See the module documentation.
fn spoken_name(node: &NodeRef<'_>) -> Option<String> {
    if node.label_comes_from_value() {
        node.value()
    } else {
        node.label()
    }
}

/// What one update makes the adapters say, gathered from the consumer's
/// change callbacks.
#[derive(Default)]
struct Heard {
    spoken: Vec<(NodeId, String, bool)>,
}

impl Heard {
    /// Record `node` if it is live and has something to say. The caller has
    /// already established that it is in the filtered tree and that this is
    /// one of the two moments an adapter speaks.
    fn hear(&mut self, node: &NodeRef<'_>) {
        let live = node.live();
        if live == Live::Off {
            return;
        }
        let Some(name) = spoken_name(node) else {
            return;
        };
        if name.trim().is_empty() {
            return;
        }
        self.spoken
            .push((node.locate().0, name, live == Live::Assertive));
    }

    /// The consumer reports changes from hash sets, so their order is
    /// arbitrary; an adapter raises its events in that same arbitrary order.
    /// Several in one update are put in the tree's reading order, so the ring
    /// is reproducible. That is the order of a walk of the replay, not of the
    /// update's node list, which the walk fills in no order a reader follows.
    fn in_reading_order(mut self, replay: &Tree) -> Vec<(String, bool)> {
        if self.spoken.len() > 1 {
            let mut position: std::collections::HashMap<NodeId, usize> =
                std::collections::HashMap::new();
            let mut stack = vec![replay.state().root()];
            while let Some(node) = stack.pop() {
                position.insert(node.locate().0, position.len());
                stack.extend(node.children().rev());
            }
            self.spoken
                .sort_by_key(|(id, _, _)| position.get(id).copied().unwrap_or(usize::MAX));
        }
        self.spoken
            .into_iter()
            .map(|(_, text, assertive)| (text, assertive))
            .collect()
    }
}

impl TreeChangeHandler for Heard {
    fn node_added(&mut self, node: &NodeRef<'_>) {
        if common_filter(node) == FilterResult::Include {
            self.hear(node);
        }
    }

    fn node_updated(&mut self, old: &NodeRef<'_>, new: &NodeRef<'_>) {
        if common_filter(new) != FilterResult::Include {
            return;
        }
        let entered = common_filter(old) != FilterResult::Include;
        if entered || spoken_name(old) != spoken_name(new) {
            self.hear(new);
        }
    }

    fn focus_moved(&mut self, _old: Option<&NodeRef<'_>>, _new: Option<&NodeRef<'_>>) {}

    fn node_removed(&mut self, _node: &NodeRef<'_>) {}
}

#[cfg(test)]
mod tests {
    use accesskit::{Live, Node, NodeId, Role, TreeId, TreeInfo, TreeUpdate};

    use super::AnnouncementRing;

    const ROOT: NodeId = NodeId(0);
    const STATUS: NodeId = NodeId(10);

    fn window_with_status(name: &str) -> TreeUpdate {
        let mut root = Node::new(Role::Window);
        root.set_children(vec![STATUS]);
        let mut status = Node::new(Role::Status);
        status.set_label(name.to_string());
        status.set_live(Live::Polite);
        TreeUpdate {
            nodes: vec![(ROOT, root), (STATUS, status)],
            tree: Some(TreeInfo::new(ROOT)),
            tree_id: TreeId::ROOT,
            focus: ROOT,
        }
    }

    fn heard(ring: &AnnouncementRing, since: u64) -> Vec<String> {
        ring.since(since).into_iter().map(|a| a.text).collect()
    }

    /// An update the consumer cannot apply panics inside it, exactly as it
    /// would inside a platform adapter. The ring is kept for automation, so it
    /// must not take the application down with it: the replay is dropped, the
    /// next update seeds a new one without announcing what was already there,
    /// and changes after that are heard again.
    #[test]
    fn an_update_the_consumer_rejects_is_survived() {
        let mut ring = AnnouncementRing::new();
        ring.observe(&window_with_status("Prêt"));
        assert_eq!(heard(&ring, 0), vec!["Prêt"]);
        let seen = 1;

        // A child named by the root that is in neither the tree nor the update.
        let mut broken = window_with_status("Prêt");
        broken.nodes[0].1.set_children(vec![STATUS, NodeId(99)]);
        ring.observe(&broken);
        assert_eq!(heard(&ring, seen), Vec::<String>::new());

        ring.observe(&window_with_status("Prêt"));
        assert_eq!(
            heard(&ring, seen),
            Vec::<String>::new(),
            "reseeding is not an arrival"
        );

        ring.observe(&window_with_status("Enregistré"));
        assert_eq!(heard(&ring, seen), vec!["Enregistré"]);
    }

    /// The ring keeps the most recent announcements and drops the oldest.
    #[test]
    fn the_ring_keeps_the_most_recent() {
        let mut ring = AnnouncementRing::new();
        for index in 0..(super::CAPACITY + 10) {
            ring.observe(&window_with_status(&format!("message {index}")));
        }
        let kept = ring.since(0);
        assert_eq!(kept.len(), super::CAPACITY);
        assert_eq!(
            kept.last().map(|a| a.text.as_str()),
            Some(format!("message {}", super::CAPACITY + 9).as_str())
        );
        assert_eq!(
            kept.first().map(|a| a.seq),
            Some(11),
            "sequence numbers keep counting past what was dropped"
        );
    }
}
