// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Speaking to a screen reader directly.
//!
//! An application often needs to say something that is not the name of any
//! widget: "Event added", "Undone: delete event", "Row moved to position 3 of
//! 12". ARIA calls the mechanism a live region, and every toolkit that has one
//! ends up building it out of a hidden node whose text changes.
//!
//! Doing that correctly is harder than it looks, and getting it wrong is
//! completely silent. This module owns the correct version so no widget and no
//! application has to.
//!
//! ## Why an application cannot do this itself
//!
//! What each platform does with a live region, read out of the three adapters
//! rather than out of anyone's documentation:
//!
//! | | node enters the filtered tree | its name changes while it stays in the tree | its name stays the same |
//! |---|---|---|---|
//! | Windows (`accesskit_windows-0.35.0`) | announces (`adapter.rs:256-263`) | announces (`adapter.rs:314-324`) | nothing |
//! | macOS (`accesskit_macos-0.27.0`) | announces (`event.rs:236-241`) | announces (`event.rs:300-310`) | nothing |
//! | Linux, AT-SPI (`accesskit_atspi_common-0.20.0`) | announces (`adapter.rs:72-77`) | announces (`node.rs:610-622`) | nothing |
//!
//! All three speak a live node as it arrives and again when its name changes,
//! and none of them speaks a name that did not change. That last column is the
//! one an application trips over: the same thing happening twice in a row ("Saved",
//! then "Saved" again) produces the same message twice, and a node that already
//! carries it has nothing new to say, so the second is never heard.
//!
//! The one mechanism that speaks a new message and a repeat of the last one
//! alike, on all three, is therefore to **put a node that was not there into
//! the filtered tree**, carrying the message: that is `add_node` on Linux and
//! the `old_filter_result != Include` arm on the other two, whatever was said
//! before.
//!
//! ## Why every message is a node the tree has never had
//!
//! Two ways of doing that fail on Linux, both measured with Orca 46.1 by
//! `tools/reader/` (the `announcer-calendar-arrows` scenario, three presses of
//! a calendar's Next month arrow):
//!
//! - **Bringing one node back.** The announcer used to own two reserved nodes
//!   and hide one between messages. `accesskit_atspi_common` announces a node
//!   that leaves the tree defunct (`adapter.rs`, `remove_node`), nothing
//!   unsays it when the same id comes back, and Orca's AT-SPI library keeps
//!   that state for the node's path: every message after the first came from a
//!   node Orca held for dead, and Orca dropped it ("Ignoring defunct object",
//!   `event_manager.py`). The first message of a session was the only one
//!   heard.
//! - **Taking a node away on the next update.** Orca does not read an
//!   announcement from the event alone: it asks the source for its role as it
//!   receives the event and again as it takes it from its queue. Behind the
//!   burst of events a calendar's month change sends, that was already after
//!   the node had been retracted, one update (about 20 ms) after it appeared:
//!   the application answered "Unknown object", and Orca dropped even a first
//!   message as defunct.
//!
//! So each message is spoken from a node id the tree has never used, drawn
//! from a range no widget or synthetic id can reach (`AnnouncerIds`), and the
//! node stays in the tree for [`LINGER`] before it leaves, never to come back.
//! Windows is owed the same time: `accesskit_windows` raises
//! `LiveRegionChanged` on the node with no text (`adapter.rs:256-263`), so a
//! UIA client has to ask the node for its name. macOS is not: its announcement
//! request carries the string (`accesskit_macos` `event.rs:65-99`).
//!
//! That is a per-platform detail an application should never have to carry, and
//! it is why this lives in the framework.
//!
//! ## Why a message waits for focus to land
//!
//! The handler that moves a row or a tab usually says where it went and puts
//! focus on it, and the move rebuilds what focus was on, so focus lands on a
//! new node. Both would reach the adapters in one update, and every adapter
//! hears an update through `accesskit_consumer`, which hands it the added and
//! changed nodes first and the focus move after them (`tree.rs:640-673`). So
//! the announcement is raised ahead of the focus event on all three:
//!
//! - **Orca 46.1** stops speaking before it reads a new focus
//!   (`locusOfFocusChanged` calls `presentationInterrupt`,
//!   `orca/scripts/default.py:698-705`). `tools/reader/` measured every such
//!   message cut ("Moved to 2 of 3", then a stop and "Documents."). An
//!   announcement that comes after the focus event is queued behind the
//!   reading instead: Orca speaks it with `interrupt=True` (`onAnnouncement`
//!   and `speakMessage`, `scripts/default.py:1424-1428, 3153`), which its
//!   speech-dispatcher backend no longer turns into a cancel
//!   (`speechdispatcherfactory.py:453-464`), at the `MESSAGE` priority that
//!   queues (`speechdispatcherfactory.py:158`).
//! - **macOS** queues the announcement request, then
//!   `FocusedUIElementChanged` (`accesskit_macos` `event.rs:236-241`,
//!   `319-326`), and posts them in that order (`event.rs:126-130`). The
//!   adapter itself raises another notification last "in order for VoiceOver
//!   to announce it" (`event.rs:273-276`), which is what a message raised
//!   first gives up.
//! - **Windows** raises `LiveRegionChanged` and then the focus change
//!   (`accesskit_windows` `adapter.rs:256-263`, `341-345`, `643-647`). NVDA
//!   keeps a live region through a focus change (see
//!   `widget_tree::accessibility_description_impl`), so it is heard there in
//!   either order; after the focus event it follows the reading of the new
//!   focus, as a browser's live region does.
//!
//! So a message never enters the tree in an update that moves focus, as the
//! consumer resolves focus (through `active_descendant`, `tree.rs:537-543`):
//! it waits, and enters the next update that leaves focus where it is. That
//! is the frame after the move, since the tree asks for one.
//!
//! ## Why the message is a `String` and not a `LocalizedString`
//!
//! Two reasons that agree. Structurally, `teksilo-i18n` depends on
//! `teksilo-core`, so this crate cannot name `LocalizedString`. Semantically, an
//! announcement is an *event*, not a label: re-resolving it when the user
//! switches language twenty minutes later would re-speak it. `tr!` resolves to a
//! `LocalizedString`, and `From<LocalizedString> for String` means
//! `ctx.announce(tr!(event_added()))` compiles and captures the wording as it
//! stood when the thing happened.
//!
//! ## Do not put an announcement beside a toast
//!
//! `Toast` is already a correct live region: it is a node that appears, which is
//! the one thing all three platforms agree on. An application that calls
//! `announce()` on the same code path as `show_toast()` says everything twice,
//! and no automatic detection is possible from either side.

use std::collections::VecDeque;
use std::time::{Duration, Instant};

use accesskit::{Live, NodeId, Role};

/// How long a message's node stays in the tree after it is spoken.
///
/// A reader asks the node for its role or name when it handles the event, not
/// when the event is sent, and a node that has gone by then is dropped as
/// defunct: Orca 46.1 behind a month change's burst of events came after a
/// one-update lifetime, and a UIA client has only the node to ask (see the
/// module documentation). Five seconds is far past that, and it is what
/// CalendrierAccessible's own voice measured working. The node then leaves so
/// that somebody walking the window later does not meet a sentence about
/// something long past.
pub const LINGER: Duration = Duration::from_secs(5);

/// How many spoken messages one politeness level keeps in the tree at once.
///
/// A burst of announcements, one a frame, would otherwise grow the tree by
/// one node a frame for [`LINGER`]. The oldest leaves early instead; it has
/// been in the tree for as many updates as there are slots, which is far
/// longer than the one update that lost messages before.
const MAX_SHOWN: usize = 8;

/// How urgently a message should interrupt.
///
/// [`Live::Off`] is deliberately not representable: a message that is never
/// spoken is not an announcement, and offering it as an option only invites
/// silence that looks like configuration.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Politeness {
    /// Spoken when the screen reader next falls idle. The right choice for
    /// almost everything: a completed action, a changed count, a status.
    #[default]
    Polite,
    /// Interrupts whatever is being spoken. For a message the user must not
    /// miss and cannot recover by re-reading the screen — a failure, a refusal,
    /// a destructive result.
    Assertive,
}

impl Politeness {
    fn live(self) -> Live {
        match self {
            Self::Polite => Live::Polite,
            Self::Assertive => Live::Assertive,
        }
    }

    /// The ARIA convention: `role="status"` for polite, `role="alert"` for
    /// assertive.
    fn role(self) -> Role {
        match self {
            Self::Polite => Role::Status,
            Self::Assertive => Role::Alert,
        }
    }
}

/// The first id the announcers speak from.
///
/// `NodeId(0)` is the tree root ([`crate::accessibility::root_node_id`]).
/// Widget-derived ids come from slotmap's `KeyData::as_ffi`, whose upper 32
/// bits hold a version counter that starts at 1, so every one of them is at
/// least `1 << 32`. Synthetic child ids always set bit 63
/// ([`crate::accessibility::SYNTHETIC_BIT`]). So `1 .. 1 << 32` belongs to
/// nobody and cannot begin to.
const FIRST_ID: u64 = 1;
/// One past the last id the announcers speak from.
const END_ID: u64 = 1 << 32;

/// Where the next message's node id comes from: one counter for both
/// politeness levels, so the two never speak from the same id, and every id
/// is used once.
///
/// Four billion messages in a tree's life before the counter would come back
/// to the start, at which point the ids it hands out were retired long before.
#[derive(Debug)]
pub(crate) struct AnnouncerIds {
    next: u64,
}

impl AnnouncerIds {
    pub(crate) fn new() -> Self {
        Self { next: FIRST_ID }
    }

    fn take(&mut self) -> NodeId {
        let id = NodeId(self.next);
        self.next = if self.next + 1 >= END_ID {
            FIRST_ID
        } else {
            self.next + 1
        };
        id
    }
}

/// One message in the tree: the node it is spoken from, and when it leaves.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Shown {
    id: NodeId,
    message: String,
    until: Instant,
}

/// One politeness level's queue and the messages it has in the tree.
///
/// Messages are queued rather than coalesced. Two things happening in quick
/// succession are two things the user needs to hear, and the alternative —
/// last-write-wins — drops the first silently.
#[derive(Debug)]
pub(crate) struct Announcer {
    politeness: Politeness,
    pending: VecDeque<String>,
    /// The message the next update is to speak, taken from `pending` with the
    /// id of the node it will be spoken from. It waits here through every
    /// update that moves focus (see the module docs).
    entering: Option<(NodeId, String)>,
    /// Messages in the tree, oldest first.
    shown: VecDeque<Shown>,
}

impl Announcer {
    pub(crate) fn new(politeness: Politeness) -> Self {
        Self {
            politeness,
            pending: VecDeque::new(),
            entering: None,
            shown: VecDeque::new(),
        }
    }

    /// Queue a message. Empty and whitespace-only messages are dropped: they
    /// would produce a node with no label, which announces nothing on any
    /// platform.
    pub(crate) fn push(&mut self, message: String) {
        if message.trim().is_empty() {
            return;
        }
        // A queue this long means something is announcing in a loop. Dropping
        // the oldest keeps the most recent state audible instead of making the
        // user wait through a backlog that is already stale.
        if self.pending.len() >= MAX_QUEUED {
            self.pending.pop_front();
        }
        self.pending.push_back(message);
    }

    /// Move to the state the `TreeUpdate` about to be built should describe,
    /// at `now` on the tree's clock.
    ///
    /// A message whose [`LINGER`] is over leaves the tree, and the next queued
    /// one is made ready to enter it from a fresh id; the update decides
    /// whether it does, and says so with [`admit`](Self::admit). One message
    /// enters an update, so two queued together are spoken in the order they
    /// were asked for: the adapters raise an update's events in no particular
    /// order.
    ///
    /// Returns whether the update about to be built may differ from the last
    /// one: a message left, or one is waiting to enter.
    pub(crate) fn step(&mut self, now: Instant, ids: &mut AnnouncerIds) -> bool {
        let before = self.shown.len();
        self.shown.retain(|shown| shown.until > now);
        if self.entering.is_none()
            && let Some(message) = self.pending.pop_front()
        {
            self.entering = Some((ids.take(), message));
        }
        self.shown.len() != before || self.entering.is_some()
    }

    /// The message ready to enter the tree, if one is.
    pub(crate) fn entering(&self) -> Option<&str> {
        self.entering.as_ref().map(|(_, message)| message.as_str())
    }

    /// The update carried the message that was ready to enter, so it is in the
    /// tree from `now` for [`LINGER`]. The oldest leaves early when the tree
    /// is full, as [`nodes`](Self::nodes) already described it.
    pub(crate) fn admit(&mut self, now: Instant) {
        let Some((id, message)) = self.entering.take() else {
            return;
        };
        if self.shown.len() >= MAX_SHOWN {
            self.shown.pop_front();
        }
        self.shown.push_back(Shown {
            id,
            message,
            until: now + LINGER,
        });
    }

    /// A message is still waiting to be spoken, so another update is owed at
    /// the next frame.
    pub(crate) fn busy(&self) -> bool {
        self.entering.is_some() || !self.pending.is_empty()
    }

    /// When the next spoken message is due to leave, so the tree can ask the
    /// event loop to wake then.
    pub(crate) fn next_retirement(&self) -> Option<Instant> {
        self.shown.iter().map(|shown| shown.until).min()
    }

    /// Shift every retirement by the distance between two clocks, when the
    /// tree moves between the wall clock and its simulated one. See
    /// [`crate::animation::AnimationScheduler::rebase`], which this mirrors.
    pub(crate) fn rebase(&mut self, from: Instant, to: Instant) {
        for shown in &mut self.shown {
            shown.until = if to >= from {
                shown.until + (to - from)
            } else {
                shown.until.checked_sub(from - to).unwrap_or(to)
            };
        }
    }

    /// The nodes to place in the `TreeUpdate`: one for each message in the
    /// tree, oldest first, and last the message ready to enter when the
    /// update is to `admit` it, which takes the oldest one's place when the
    /// tree is full.
    pub(crate) fn nodes(
        &self,
        admit: bool,
    ) -> impl Iterator<Item = (NodeId, accesskit::Node)> + '_ {
        let entering = self.entering.as_ref().filter(|_| admit);
        let full = entering.is_some() && self.shown.len() >= MAX_SHOWN;
        self.shown
            .iter()
            .skip(usize::from(full))
            .map(|shown| (shown.id, shown.message.as_str()))
            .chain(entering.map(|(id, message)| (*id, message.as_str())))
            .map(|(id, message)| {
                let mut node = accesskit::Node::new(self.politeness.role());
                node.set_live(self.politeness.live());
                // The label, not the value. `accesskit_consumer`'s
                // `label_comes_from_value` is true for `Role::Label` and
                // nothing else (node.rs:744-746), so every adapter reads the
                // announced text from `label()`, and a live region that sets
                // only `value` is silent on all three platforms.
                node.set_label(message);
                (id, node)
            })
    }
}

/// Whether `id` is one the announcers speak through.
///
/// They hang directly off the root, where no clipping parent can move them out
/// of view, which is what lets the announcement ring skip a scroll that has
/// nothing else live to move.
pub(crate) fn is_announcer_node(id: NodeId) -> bool {
    (FIRST_ID..END_ID).contains(&id.0)
}

/// How many messages one politeness level will hold before dropping the oldest.
const MAX_QUEUED: usize = 32;

#[cfg(test)]
mod tests {
    use super::*;

    fn at(start: Instant, millis: u64) -> Instant {
        start + Duration::from_millis(millis)
    }

    fn labels(a: &Announcer) -> Vec<String> {
        a.nodes(false)
            .map(|(_, node)| node.label().unwrap_or_default().to_string())
            .collect()
    }

    fn ids(a: &Announcer) -> Vec<NodeId> {
        a.nodes(false).map(|(id, _)| id).collect()
    }

    /// One update that leaves focus where it is, so the message ready to
    /// enter does, as the tree drives it. Returns what `step` said.
    fn update(a: &mut Announcer, now: Instant, ids: &mut AnnouncerIds) -> bool {
        let changed = a.step(now, ids);
        a.admit(now);
        changed
    }

    #[test]
    fn an_idle_announcer_puts_nothing_in_the_tree() {
        let mut ids_ = AnnouncerIds::new();
        let mut a = Announcer::new(Politeness::Polite);
        assert!(!update(&mut a, Instant::now(), &mut ids_));
        assert_eq!(a.next_retirement(), None);
        assert_eq!(a.nodes(true).count(), 0);
    }

    /// A message enters the tree from a fresh node, stays for [`LINGER`], and
    /// leaves.
    #[test]
    fn a_message_stays_for_its_linger_and_then_leaves() {
        let start = Instant::now();
        let mut ids_ = AnnouncerIds::new();
        let mut a = Announcer::new(Politeness::Polite);
        a.push("Event added".to_string());

        assert!(update(&mut a, start, &mut ids_) && !a.busy());
        assert_eq!(a.next_retirement(), Some(start + LINGER));
        assert_eq!(labels(&a), ["Event added"]);

        assert!(
            !update(&mut a, start + LINGER - Duration::from_millis(1), &mut ids_),
            "nothing changes while the message lingers"
        );
        assert_eq!(labels(&a), ["Event added"]);

        assert!(update(&mut a, start + LINGER, &mut ids_));
        assert_eq!(a.next_retirement(), None);
        assert_eq!(a.nodes(true).count(), 0);
    }

    /// The case the whole mechanism exists for. All three platforms announce a
    /// node entering; none announces the *same* label written twice; and on
    /// AT-SPI a node that has left once is defunct for good. So the second
    /// "Saved" must come from a node that has never been in the tree.
    #[test]
    fn the_same_message_twice_is_two_new_nodes() {
        let start = Instant::now();
        let mut ids_ = AnnouncerIds::new();
        let mut a = Announcer::new(Politeness::Polite);
        a.push("Saved".to_string());
        update(&mut a, start, &mut ids_);
        let first = ids(&a);
        update(&mut a, start + LINGER, &mut ids_);
        a.push("Saved".to_string());
        update(&mut a, at(start + LINGER, 1), &mut ids_);
        let second = ids(&a);
        assert_eq!(labels(&a), ["Saved"]);
        assert_eq!(first.len(), 1);
        assert_eq!(second.len(), 1);
        assert_ne!(
            first, second,
            "a message must never be spoken from an id used before"
        );
    }

    /// Two things happening in quick succession are two things to say, one an
    /// update, in order, and the first stays while the second is spoken.
    #[test]
    fn messages_queue_one_an_update_and_keep_their_order() {
        let start = Instant::now();
        let mut ids_ = AnnouncerIds::new();
        let mut a = Announcer::new(Politeness::Polite);
        a.push("first".to_string());
        a.push("second".to_string());

        update(&mut a, start, &mut ids_);
        assert!(a.busy(), "the second message is owed an update of its own");
        assert_eq!(labels(&a), ["first"]);

        update(&mut a, at(start, 16), &mut ids_);
        assert!(!a.busy());
        assert_eq!(labels(&a), ["first", "second"]);
        assert_eq!(
            a.next_retirement(),
            Some(start + LINGER),
            "the earliest leaves first"
        );
    }

    /// A message that waits through an update (one that moved focus) keeps
    /// the id it was given and its place in the queue, and lingers from when
    /// it enters, not from when it was asked for.
    #[test]
    fn a_waiting_message_keeps_its_id_and_its_place() {
        let start = Instant::now();
        let mut ids_ = AnnouncerIds::new();
        let mut a = Announcer::new(Politeness::Polite);
        a.push("first".to_string());
        a.push("second".to_string());

        assert!(a.step(start, &mut ids_));
        let ready: Vec<NodeId> = a.nodes(true).map(|(id, _)| id).collect();
        assert_eq!(a.nodes(false).count(), 0, "held, so nothing in the tree");

        assert!(
            a.step(at(start, 16), &mut ids_),
            "a waiting message owes the next update a walk"
        );
        assert_eq!(a.entering(), Some("first"), "the second does not jump it");
        assert_eq!(
            a.nodes(true).map(|(id, _)| id).collect::<Vec<_>>(),
            ready,
            "the id it was given is the one it enters with"
        );
        a.admit(at(start, 16));
        assert_eq!(labels(&a), ["first"]);
        assert_eq!(a.next_retirement(), Some(at(start, 16) + LINGER));
        assert!(a.busy(), "the second is still owed an update");

        update(&mut a, at(start, 32), &mut ids_);
        assert_eq!(labels(&a), ["first", "second"]);
        assert!(!a.busy());
    }

    #[test]
    fn an_empty_message_is_dropped() {
        let mut ids_ = AnnouncerIds::new();
        let mut a = Announcer::new(Politeness::Polite);
        a.push(String::new());
        a.push("   \n\t ".to_string());
        assert!(!update(&mut a, Instant::now(), &mut ids_));
    }

    #[test]
    fn a_runaway_queue_drops_the_oldest_not_the_newest() {
        let mut ids_ = AnnouncerIds::new();
        let mut a = Announcer::new(Politeness::Polite);
        for i in 0..(MAX_QUEUED + 5) {
            a.push(format!("message {i}"));
        }
        update(&mut a, Instant::now(), &mut ids_);
        assert_eq!(
            labels(&a),
            ["message 5"],
            "the five oldest must be the ones dropped"
        );
    }

    /// A burst does not grow the tree past [`MAX_SHOWN`]: the oldest leaves
    /// early, and every message is still spoken in turn.
    #[test]
    fn a_burst_keeps_at_most_max_shown_in_the_tree() {
        let start = Instant::now();
        let mut ids_ = AnnouncerIds::new();
        let mut a = Announcer::new(Politeness::Polite);
        for i in 0..20 {
            a.push(format!("row {i}"));
        }
        for frame in 0..20 {
            a.step(at(start, frame * 16), &mut ids_);
            assert!(
                a.nodes(true).count() <= MAX_SHOWN,
                "the update that admits a message into a full tree carries no more"
            );
            a.admit(at(start, frame * 16));
            assert!(a.nodes(false).count() <= MAX_SHOWN);
        }
        let last: Vec<String> = (12..20).map(|i| format!("row {i}")).collect();
        assert_eq!(labels(&a), last);
    }

    /// Both politeness levels draw from one counter, so they never share an
    /// id, and no id is ever one the tree gives a widget, a synthetic child or
    /// the root.
    #[test]
    fn announcer_ids_are_unique_and_outside_every_other_range() {
        let start = Instant::now();
        let mut ids_ = AnnouncerIds::new();
        let mut polite = Announcer::new(Politeness::Polite);
        let mut assertive = Announcer::new(Politeness::Assertive);
        let mut seen = std::collections::HashSet::new();
        for i in 0..20 {
            polite.push(format!("p{i}"));
            assertive.push(format!("a{i}"));
            update(&mut polite, at(start, i * 16), &mut ids_);
            update(&mut assertive, at(start, i * 16), &mut ids_);
            for id in ids(&polite).into_iter().chain(ids(&assertive)) {
                seen.insert(id);
                assert!(is_announcer_node(id));
                assert_ne!(id, crate::accessibility::root_node_id());
                assert!(!crate::accessibility::is_synthetic(id));
                assert!(id.0 < 1 << 32, "a widget id is at least 1 << 32");
            }
        }
        assert_eq!(seen.len(), 40, "forty messages, forty ids");
    }

    #[test]
    fn the_counter_wraps_inside_its_range() {
        let mut ids_ = AnnouncerIds { next: END_ID - 1 };
        assert_eq!(ids_.take(), NodeId(END_ID - 1));
        assert_eq!(ids_.take(), NodeId(FIRST_ID));
    }

    /// Moving between the wall clock and the simulated one shifts when a
    /// message leaves, as it shifts an animation: the message keeps the time
    /// it had left.
    #[test]
    fn a_rebase_keeps_the_time_a_message_has_left() {
        let start = Instant::now();
        let mut ids_ = AnnouncerIds::new();
        let mut a = Announcer::new(Politeness::Polite);
        a.push("Saved".to_string());
        update(&mut a, start, &mut ids_);
        let later = start + Duration::from_secs(100);
        a.rebase(start, later);
        assert!(!update(
            &mut a,
            later + LINGER - Duration::from_millis(1),
            &mut ids_
        ));
        assert_eq!(labels(&a), ["Saved"]);
        update(&mut a, later + LINGER, &mut ids_);
        assert_eq!(a.nodes(true).count(), 0);
    }

    #[test]
    fn politeness_maps_to_the_aria_role_and_live_setting() {
        let mut ids_ = AnnouncerIds::new();
        let mut polite = Announcer::new(Politeness::Polite);
        polite.push("p".to_string());
        update(&mut polite, Instant::now(), &mut ids_);
        let (_, node) = polite.nodes(false).next().unwrap();
        assert_eq!(node.role(), Role::Status);
        assert_eq!(node.live(), Some(Live::Polite));

        let mut assertive = Announcer::new(Politeness::Assertive);
        assertive.push("a".to_string());
        update(&mut assertive, Instant::now(), &mut ids_);
        let (_, node) = assertive.nodes(false).next().unwrap();
        assert_eq!(node.role(), Role::Alert);
        assert_eq!(node.live(), Some(Live::Assertive));
    }
}
