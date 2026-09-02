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
//! | | node enters the filtered tree | its label changes while it stays in the tree |
//! |---|---|---|
//! | Windows (`accesskit_windows-0.35.0`) | announces (`adapter.rs:256-263`) | announces (`adapter.rs:314-324`) |
//! | macOS (`accesskit_macos-0.27.0`) | announces (`event.rs:236-241`) | announces (`event.rs:300-310`) |
//! | Linux, AT-SPI (`accesskit_atspi_common-0.20.0`) | announces (`adapter.rs:72-77`) | **never** |
//!
//! The AT-SPI adapter emits `ObjectEvent::Announcement` from exactly one place,
//! `add_node`. Its `node_updated` compares interfaces, bounds, text and
//! selection, and says nothing at all about `live`. So on Linux, changing a
//! live region's text announces **nothing** — the only thing that speaks is a
//! node arriving in the filtered tree.
//!
//! Meanwhile on Windows and macOS a *repeated* message does not announce
//! either, because both adapters require the label to have changed.
//!
//! The single mechanism that satisfies all three, for both a new message and a
//! repeat of the previous one, is therefore to **retract the node and put it
//! back**: hide it, then re-expose it carrying the message. `common_filter`
//! turns `is_hidden` into `FilterResult::ExcludeSubtree`
//! (`accesskit_consumer-0.39.0/src/filters.rs:22-24`), so hiding removes the
//! node from the filtered tree and un-hiding is a genuine re-entry — which is
//! `add_node` on Linux and the `old_filter_result != Include` arm on the other
//! two.
//!
//! That is a per-platform detail an application should never have to carry, and
//! it is why this lives in the framework.
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

use accesskit::{Live, NodeId, Role};

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

    /// The reserved node this politeness level speaks through.
    ///
    /// `NodeId(0)` is the tree root ([`crate::accessibility::root_node_id`]).
    /// Widget-derived ids come from slotmap's `KeyData::as_ffi`, whose upper 32
    /// bits hold a version counter that starts at 1, so every one of them is at
    /// least `1 << 32`. Synthetic child ids always set bit 63
    /// ([`crate::accessibility::SYNTHETIC_BIT`]). So 1 and 2 belong to nobody
    /// and cannot begin to.
    fn node_id(self) -> NodeId {
        match self {
            Self::Polite => NodeId(1),
            Self::Assertive => NodeId(2),
        }
    }
}

/// Where one politeness level's announcer is in its expose / retract cycle.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Phase {
    /// Nothing to say. The node is emitted hidden and unlabelled.
    Idle,
    /// This message is being spoken: the node is emitted visible, carrying it.
    Speaking(String),
    /// The message has been delivered. The node is emitted hidden again, so
    /// that the next one is a genuine re-entry rather than a label edit.
    Retracting,
}

/// One politeness level's queue and cycle position.
///
/// Messages are queued rather than coalesced. Two things happening in quick
/// succession are two things the user needs to hear, and the alternative —
/// last-write-wins — drops the first silently.
#[derive(Debug)]
pub(crate) struct Announcer {
    politeness: Politeness,
    phase: Phase,
    pending: VecDeque<String>,
}

impl Announcer {
    pub(crate) fn new(politeness: Politeness) -> Self {
        Self {
            politeness,
            phase: Phase::Idle,
            pending: VecDeque::new(),
        }
    }

    /// Queue a message. Empty and whitespace-only messages are dropped: they
    /// would produce a node with no label, which announces nothing on any
    /// platform, while still costing a full expose / retract cycle.
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

    /// Move to the state the `TreeUpdate` about to be built should describe.
    ///
    /// Returns `true` when a *further* update is needed after this one, so the
    /// caller knows to ask for another accessibility sync and another frame.
    /// Every message costs exactly two updates: one that exposes its node, one
    /// that retracts it. Retracting is not optional — it is what makes the next
    /// message a re-entry into the filtered tree, which on Linux is the only
    /// thing that announces at all.
    pub(crate) fn step(&mut self) -> bool {
        self.phase = match std::mem::replace(&mut self.phase, Phase::Idle) {
            // This update exposed a message; the next one has to take it away.
            Phase::Speaking(_) => Phase::Retracting,
            // Idle or Retracting: both emit the same hidden node, so both are
            // free to pick up the next message.
            _ => match self.pending.pop_front() {
                Some(message) => Phase::Speaking(message),
                None => Phase::Idle,
            },
        };
        // A retract that has nothing queued behind it is the last update this
        // announcer needs: `Idle` emits exactly what `Retracting` does.
        matches!(self.phase, Phase::Speaking(_)) || !self.pending.is_empty()
    }

    /// The node to place in the `TreeUpdate` for the current phase.
    pub(crate) fn node(&self) -> (NodeId, accesskit::Node) {
        let mut node = accesskit::Node::new(self.politeness.role());
        node.set_live(self.politeness.live());
        match &self.phase {
            Phase::Speaking(message) => {
                // The label, not the value. `accesskit_consumer`'s
                // `label_comes_from_value` is true for `Role::Label` and
                // nothing else (node.rs:744-746), so every adapter reads the
                // announced text from `label()`. A live region that sets only
                // `value` is silent on all three platforms while an in-process
                // test that reads `value().or(label())` still passes it.
                node.set_label(message.clone());
            }
            Phase::Idle | Phase::Retracting => {
                node.set_hidden();
            }
        }
        (self.politeness.node_id(), node)
    }
}

/// How many messages one politeness level will hold before dropping the oldest.
const MAX_QUEUED: usize = 32;

#[cfg(test)]
mod tests {
    use super::*;

    fn label_of(a: &Announcer) -> Option<String> {
        let (_, node) = a.node();
        node.label().map(|s| s.to_string())
    }

    fn hidden(a: &Announcer) -> bool {
        let (_, node) = a.node();
        node.is_hidden()
    }

    fn node_id(a: &Announcer) -> NodeId {
        a.node().0
    }

    #[test]
    fn an_idle_announcer_is_hidden_and_unlabelled() {
        let mut a = Announcer::new(Politeness::Polite);
        assert!(hidden(&a));
        assert_eq!(label_of(&a), None);
        assert!(!a.step(), "an idle announcer has nothing to do");
    }

    /// One message costs two syncs: expose, then retract. The retract is not
    /// optional — it is what makes the *next* message a re-entry into the
    /// filtered tree, which on Linux is the only thing that announces at all.
    #[test]
    fn a_message_is_exposed_then_retracted() {
        let mut a = Announcer::new(Politeness::Polite);
        a.push("Event added".to_string());

        assert!(a.step(), "still busy: the retract is yet to come");
        assert!(!hidden(&a), "the node must enter the filtered tree");
        assert_eq!(label_of(&a).as_deref(), Some("Event added"));

        assert!(!a.step(), "nothing left after the retract");
        assert!(hidden(&a), "the node must leave the filtered tree again");
    }

    /// The case the whole retract mechanism exists for. Windows and macOS
    /// announce a label change; neither announces the *same* label written
    /// twice. Hiding in between makes the second one a re-entry, which all
    /// three platforms speak.
    #[test]
    fn the_same_message_twice_is_exposed_twice() {
        let mut a = Announcer::new(Politeness::Polite);
        a.push("Saved".to_string());
        a.push("Saved".to_string());

        a.step();
        assert!(!hidden(&a));
        assert_eq!(label_of(&a).as_deref(), Some("Saved"));

        a.step();
        assert!(hidden(&a), "the node must be retracted between the two");

        a.step();
        assert!(!hidden(&a));
        assert_eq!(label_of(&a).as_deref(), Some("Saved"));

        assert!(!a.step());
        assert!(hidden(&a));
    }

    /// Two things happening in quick succession are two things to say. A
    /// coalescing announcer would drop the first without a trace.
    #[test]
    fn messages_queue_rather_than_replace_one_another() {
        let mut a = Announcer::new(Politeness::Polite);
        a.push("first".to_string());
        a.push("second".to_string());

        let mut spoken = Vec::new();
        for _ in 0..4 {
            a.step();
            if let Some(l) = label_of(&a) {
                spoken.push(l);
            }
        }
        assert_eq!(spoken, vec!["first", "second"]);
    }

    #[test]
    fn an_empty_message_is_dropped_rather_than_costing_a_cycle() {
        let mut a = Announcer::new(Politeness::Polite);
        a.push(String::new());
        a.push("   \n\t ".to_string());
        assert!(!a.step());
    }

    #[test]
    fn a_runaway_queue_drops_the_oldest_not_the_newest() {
        let mut a = Announcer::new(Politeness::Polite);
        for i in 0..(MAX_QUEUED + 5) {
            a.push(format!("message {i}"));
        }
        a.step();
        assert_eq!(
            label_of(&a).as_deref(),
            Some("message 5"),
            "the five oldest must be the ones dropped"
        );
    }

    #[test]
    fn the_two_politeness_levels_use_distinct_reserved_nodes() {
        let polite = node_id(&Announcer::new(Politeness::Polite));
        let assertive = node_id(&Announcer::new(Politeness::Assertive));
        assert_ne!(polite, assertive);
        assert_ne!(polite, crate::accessibility::root_node_id());
        assert_ne!(assertive, crate::accessibility::root_node_id());
        assert!(!crate::accessibility::is_synthetic(polite));
        assert!(!crate::accessibility::is_synthetic(assertive));
    }

    #[test]
    fn politeness_maps_to_the_aria_role_and_live_setting() {
        let (_, polite) = Announcer::new(Politeness::Polite).node();
        assert_eq!(polite.role(), Role::Status);
        assert_eq!(polite.live(), Some(Live::Polite));

        let (_, assertive) = Announcer::new(Politeness::Assertive).node();
        assert_eq!(assertive.role(), Role::Alert);
        assert_eq!(assertive.live(), Some(Live::Assertive));
    }
}
