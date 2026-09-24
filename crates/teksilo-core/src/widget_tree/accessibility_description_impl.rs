// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! A node's description, computed from the nodes it is `described_by`.
//!
//! ## Why the framework computes it
//!
//! A `described_by` relation reaches no screen reader through AccessKit 0.25.
//! `accesskit_consumer-0.39.0` gives a node a description from its own
//! `description` property and nothing else (`node.rs:796-800`), unlike its name,
//! which it does assemble from `labelled_by` (`node.rs:774-793`). And no adapter
//! exports the relation: the AT-SPI relation set holds `ControllerFor` alone
//! (`accesskit_atspi_common-0.20.0/src/node.rs:960-977`), UIA answers
//! `ControllerFor` and never `DescribedBy` (`accesskit_windows-0.35.0/src/node.rs:
//! 1044`), and macOS links only `controls` (`accesskit_macos-0.27.0/src/node.rs:
//! 1161-1176`). So a field that points at its validation message is read as if it
//! pointed at nothing. This pass does what a browser's accessible-description
//! computation does with `aria-describedby`: it writes the text of every target
//! onto the node's `description`, after the node's own. That property is what
//! each platform reads: AT-SPI `Description` (`accesskit_atspi_common` `node.rs:
//! 46-48, 835-840`), UIA `FullDescription` (`accesskit_windows` `node.rs:415-421,
//! 1311`), macOS `accessibilityHelp` (`accesskit_macos` `node.rs:543-550`).
//!
//! The relation is kept beside it. Orca, the one reader that follows it, reads
//! the relation's targets only when the description is empty (Orca 46.1,
//! `orca/generator.py:463-464`: `get_description(obj) or
//! displayedDescription(obj)`), so a node carrying both is read once.
//!
//! ## What the readers do with a description
//!
//! - **Orca 46.1** reads it as focus arrives: a node newly focused is spoken
//!   in the `unfocused` format (`orca/generator.py:227-231`), whose suffix
//!   includes `description` (`orca/formatting.py:129`), with
//!   `speakDescription` on by default (`orca/settings.py:243`).
//!   It also **speaks a change to it** on the focused node, with no check that
//!   the text is new: `onDescriptionChanged` compares against
//!   `pointOfReference['description']`, a key nothing writes (the focus handler
//!   stores `'descriptions'`), and presents any non-empty text
//!   (`orca/scripts/default.py:1316-1335`). Changes on other nodes are ignored.
//! - **NVDA** (source at nvaccess/nvda `f56b2e0d`) takes a UIA element's
//!   description from `FullDescription`, falling back to `HelpText`
//!   (`NVDAObjects/UIA/__init__.py` `_get_description`), and reports it on focus
//!   (`presentation.reportObjectDescriptions`, default on). It maps no event to a
//!   `FullDescription` change (`UIAHandler/__init__.py`
//!   `UIAPropertyIdsToNVDAEventNames` lists `HelpText` only), so a change is not
//!   spoken.
//! - **VoiceOver** is closed source. It reads `AXHelp` as a hint, after a pause
//!   or on VO-Shift-H; that is documented behaviour, not verified here.
//!   `accesskit_macos` posts no notification when a description changes
//!   (`event.rs` `node_updated` posts title, value and selected-text changes).
//!
//! ## The order an update arrives in
//!
//! Every adapter hears an update through `accesskit_consumer`, which reports
//! the nodes added and changed first and the focus move after them
//! (`tree.rs:640-673`). So an announcement, and a description change on the
//! node focus is leaving, both reach a reader before it learns where focus
//! went. What the readers then do with them differs:
//!
//! - **Orca 46.1** stops speaking before it reads the new focus:
//!   `locusOfFocusChanged` calls `presentationInterrupt`
//!   (`orca/scripts/default.py:698-705`), which is `speech.stop()` and so a
//!   speech-dispatcher cancel (`orca/scripts/default.py:2674-2682`,
//!   `orca/speech.py:309-311`, `orca/speechdispatcherfactory.py:358-359`),
//!   whenever `shouldInterruptForLocusOfFocusChange` says so, and for a focus
//!   event it does unless one node is the other's named ancestor or controls
//!   it (`orca/script_utilities.py:4125-4171`). An announcement made in the
//!   update focus moves in is cut before it is heard. Orca still holds the
//!   node focus is leaving as its focus while it handles that node's changes,
//!   so it speaks a description change there (the check in
//!   `orca/scripts/default.py:1316-1335` is against the focus it has, not the
//!   one the update carries), and cuts that too. Read from the source, not
//!   heard from a running Orca.
//! - **NVDA** does not stop: `event_gainFocus` reads the node with
//!   `speakObject` and cancels nothing (`NVDAObjects/__init__.py:1204-1206,
//!   1365-1372`; `eventHandler.py` `doPreGainFocus` drops only speech queued
//!   for an earlier focus), and a live region is a `ui.message`
//!   (`NVDAObjects/__init__.py:1238-1253`). Both are heard, in order.
//!
//! ## Why a description does not always take the text at once
//!
//! A description is for arriving; a change is for a live region. Written
//! naively, the text reaches a reader twice, or not at all, in the moments that
//! matter most:
//!
//! - a field validates while the user is still in it (Enter, or an app setting
//!   the error), its `ValidationStrip` announces the message, and the focused
//!   field's description changes to the same message, which Orca speaks again;
//! - the user leaves that field, the message the description held back joins
//!   it in the update focus moves in, and Orca starts saying it once more as
//!   it leaves, only to cut it for the next field;
//! - focus is sent to the field at fault in the same update the message is
//!   announced (the WCAG pattern of focusing the first error). NVDA says the
//!   announcement and then the field, so a description holding the message too
//!   says it twice; Orca cuts the announcement, so a description without the
//!   message leaves nothing that says it.
//!
//! So:
//!
//! 1. **Only where the reader keeps an announcement through a focus move**, a
//!    text some live region is announcing in this very update (a live node the
//!    adapters see newly, re-worded or with a new politeness, the framework's
//!    announcer included) is left out of the node focus is on and every node
//!    it is inside. That is Windows, where NVDA keeps it. On AT-SPI Orca cuts
//!    it, so the arrival is the one voice left and keeps the text. VoiceOver
//!    is unverified and is treated as Orca is: a text left out of the arrival
//!    and cut from the announcement is not heard at all, while one kept is at
//!    worst heard twice, the second time as a hint after a pause.
//! 2. **A node focus was on or inside in the last delivered update** gains no
//!    text in this one: not while focus stays, where Orca speaks the change,
//!    and not as focus leaves it, where Orca starts to. It may lose one: a
//!    message that has gone away is dropped, never read out as stale. A node
//!    focus has left takes what it held back in the next update, which the
//!    tree asks for itself; by then Orca's focus is elsewhere.
//!
//! A left-out text arrives the next time focus does. Rule 1 defers to a voice
//! saying the text at that moment, where that voice is known to be heard.
//! Rule 2 costs only a change Orca alone would have spoken, so on every
//! platform a message that must be heard as it appears needs a live region,
//! and one without it is heard on the next arrival.

use std::collections::{HashMap, HashSet};

use accesskit::{Live, Node, NodeId, Role};

use super::WidgetTree;

/// What a platform's screen reader does with an announcement made in the
/// update focus moves in, which decides rule 1 (see the module docs).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum AnnouncementOnFocusMove {
    /// Heard, and the new focus read after it: NVDA, so Windows. The arrival
    /// leaves the announced text out, or it would be said twice.
    Kept,
    /// Cut by the read of the new focus: Orca, so every AT-SPI platform, and
    /// VoiceOver, which is unverified and treated the same way because a text
    /// left out of the arrival would then be heard by nobody. The arrival
    /// keeps the text, as the one voice left.
    Cut,
}

impl AnnouncementOnFocusMove {
    /// What this build's platform does. A compile-time choice because
    /// AccessKit does not say which reader is listening, and each platform
    /// has the one reader this is known for.
    pub(super) const PLATFORM: Self = if cfg!(target_os = "windows") {
        Self::Kept
    } else {
        Self::Cut
    };
}

impl Default for AnnouncementOnFocusMove {
    fn default() -> Self {
        Self::PLATFORM
    }
}

/// What the last delivered update said, as far as this pass needs to know.
///
/// Kept by [`WidgetTree::sync_accessibility`], which is the only caller whose
/// update reaches a screen reader; a snapshot reads it and never writes it.
#[derive(Debug, Default)]
pub(super) struct DescriptionMemory {
    /// Carried from one update to the next unchanged. A field rather than
    /// [`AnnouncementOnFocusMove::PLATFORM`] read in place so that a test can
    /// hold a tree to either reader's rule on any host.
    pub(super) on_focus_move: AnnouncementOnFocusMove,
    /// The description parts of every node focus was on or inside, in order.
    /// A node is a key exactly when it was on that path, even with no parts,
    /// because rule 2 asks whether focus was there.
    focus_parts: HashMap<NodeId, Vec<String>>,
    /// The text and politeness of every live node the adapters could see,
    /// which is what they compare against to decide whether a live node
    /// announces.
    live_texts: HashMap<NodeId, (String, Live)>,
    /// A node focus has just left held back a text, which it takes in the
    /// next update; `sync_accessibility` asks for that update, since nothing
    /// else may.
    pub(super) catching_up: bool,
}

impl WidgetTree {
    /// Write each `described_by` node's description from its targets, and
    /// answer what the update now says for the next one to be measured
    /// against. See the module docs for the rules.
    ///
    /// Runs last, on the nodes the update will carry: relation targets are
    /// already redirected to proxies and stripped of absent nodes, so every
    /// target named here is in `nodes`.
    pub(super) fn describe_from_relations(
        &self,
        nodes: &mut [(NodeId, Node)],
        focus: NodeId,
    ) -> DescriptionMemory {
        let previous = &self.description_memory;
        let index: HashMap<NodeId, usize> = nodes
            .iter()
            .enumerate()
            .map(|(i, (id, _))| (*id, i))
            .collect();
        let mut parent: HashMap<NodeId, NodeId> = HashMap::new();
        for (id, node) in nodes.iter() {
            for &child in node.children() {
                parent.entry(child).or_insert(*id);
            }
        }
        let node = |id: NodeId| index.get(&id).map(|&i| &nodes[i].1);

        // Hidden or under a hidden node: out of the tree every adapter reads
        // (`common_filter` drops a hidden node with its whole subtree), so a
        // live region there says nothing.
        let unseen = |id: NodeId| {
            let mut current = Some(id);
            while let Some(id) = current {
                if node(id).is_some_and(|n| n.is_hidden()) {
                    return true;
                }
                current = parent.get(&id).copied();
            }
            false
        };

        // The politeness a node is announced at, and whether it set it
        // itself: its own, or failing that its nearest ancestor's, which is
        // how `accesskit_consumer`'s `NodeRef::live` resolves it
        // (`node.rs:906-910`). Every named node inside a live region is live.
        let politeness = |id: NodeId| {
            let mut current = Some(id);
            while let Some(at) = current {
                if let Some(live) = node(at).and_then(Node::live) {
                    return (live, at == id);
                }
                current = parent.get(&at).copied();
            }
            (Live::Off, false)
        };

        // Every live node the adapters see, and among them the ones speaking
        // now. All three announce a live node that enters the tree or whose
        // name changes (`accesskit_atspi_common-0.20.0` `adapter.rs:72-77`,
        // `node.rs:610-622`; `accesskit_windows-0.35.0` `adapter.rs:256-263,
        // 313-324`; `accesskit_macos-0.27.0` `event.rs:300-310`), by the name
        // they compute, which is a value for a `Role::Label` alone. Windows
        // also announces one whose politeness alone changes
        // (`adapter.rs:313-324`), so that counts as speaking where rule 1
        // applies, but only on the node that sets it: the consumer reports a
        // node as changed when its own data changes, and a region's new
        // politeness changes nothing in the nodes that inherit it. The texts
        // are kept on every platform, since the next update is measured
        // against them.
        let mut live_texts: HashMap<NodeId, (String, Live)> = HashMap::new();
        let mut announcing: HashSet<String> = HashSet::new();
        for (id, n) in nodes.iter() {
            // A `GenericContainer` or a text run is passed over by
            // `common_filter` whatever it is named, unless it holds focus
            // (consumer `filters.rs:17-34`), and an adapter announces only a
            // node the filter includes. What it holds still inherits its
            // politeness, which `politeness` resolves through it.
            if *id != focus && matches!(n.role(), Role::GenericContainer | Role::TextRun) {
                continue;
            }
            let (live, own) = politeness(*id);
            if live == Live::Off || unseen(*id) {
                continue;
            }
            let Some(text) = spoken_name(n) else { continue };
            let now = (text.to_owned(), live);
            let speaking = match previous.live_texts.get(id) {
                None => true,
                Some((was, was_live)) => *was != now.0 || (own && *was_live != live),
            };
            if speaking {
                announcing.insert(now.0.clone());
            }
            live_texts.insert(*id, now);
        }

        // The node focus is on and every node it is inside, which are the
        // nodes a reader describes as focus arrives.
        let mut on_focus_path: HashSet<NodeId> = HashSet::new();
        let mut current = Some(focus);
        while let Some(id) = current {
            if !on_focus_path.insert(id) {
                break;
            }
            current = parent.get(&id).copied();
        }

        let mut focus_parts: HashMap<NodeId, Vec<String>> = HashMap::new();
        let mut written: Vec<(usize, String)> = Vec::new();
        let mut catching_up = false;
        let announcement_kept = previous.on_focus_move == AnnouncementOnFocusMove::Kept;
        for (i, (id, n)) in nodes.iter().enumerate() {
            let focused = on_focus_path.contains(id);
            if n.described_by().is_empty() && !focused {
                continue;
            }
            // On or inside the focus the last update carried: this node is
            // the focus a reader still holds, or one around it.
            let before = previous.focus_parts.get(id);
            let own = n
                .description()
                .map(str::trim)
                .filter(|text| !text.is_empty());
            let name = own_text(n);
            let mut parts: Vec<String> = own.map(str::to_owned).into_iter().collect();
            let mut derived = false;
            for &target in n.described_by() {
                let Some(text) = node(target).and_then(|t| relation_text(t, &node)) else {
                    continue;
                };
                // Said already: as the name, which every reader says first,
                // or as an earlier part.
                if name == Some(text.as_str()) || parts.contains(&text) {
                    continue;
                }
                // Rule 1: a live region is saying it right now, to a reader
                // that goes on saying it as focus lands here.
                if focused && announcement_kept && announcing.contains(&text) {
                    continue;
                }
                // Rule 2: focus was here, and the text is new to the node.
                if before.is_some_and(|before| !before.contains(&text)) {
                    catching_up |= !focused;
                    continue;
                }
                parts.push(text);
                derived = true;
            }
            if derived {
                written.push((i, parts.join(" ")));
            }
            if focused {
                focus_parts.insert(*id, parts);
            }
        }
        for (i, description) in written {
            nodes[i].1.set_description(description);
        }

        DescriptionMemory {
            on_focus_move: previous.on_focus_move,
            focus_parts,
            live_texts,
            catching_up,
        }
    }
}

/// The name every adapter announces for a live node: the value of a
/// `Role::Label` and the label of anything else, and neither in place of the
/// other (`accesskit_consumer` `node.rs:744-746`; see
/// [`crate::accessibility::announced_text`]). A name drawn through
/// `labelled_by` is not followed here, so a live region named that way is not
/// taken to be speaking, and rule 1 leaves its text in the arrival.
fn spoken_name(node: &Node) -> Option<&str> {
    crate::accessibility::announced_text(node)
        .map(str::trim)
        .filter(|text| !text.is_empty())
}

/// The text a node carries, for a description to quote: its name, or for a
/// node named from its value (a `Role::Label`, per `accesskit_consumer`
/// `node.rs:744-746`) its value, and failing that the other of the two. This
/// is what a target shows, not what an adapter would announce for it, which
/// is [`spoken_name`].
fn own_text(node: &Node) -> Option<&str> {
    let text = if node.role() == Role::Label {
        node.value().or_else(|| node.label())
    } else {
        node.label().or_else(|| node.value())
    };
    text.map(str::trim).filter(|text| !text.is_empty())
}

/// The text a relation target contributes: its own, or failing that the text
/// of what it contains, in reading order.
///
/// As in the accessible-name computation a browser runs for
/// `aria-describedby`, a target counts even when it is hidden, since pointing
/// at hidden text is how a description is kept off the reading path; what a
/// target contains counts only when it is not hidden. Text runs are skipped.
/// A node with text of its own stops the walk before its runs are reached, so
/// the runs met here belong to a node that carries its text in runs alone,
/// which is a document surface (the code editor, the log): its whole visible
/// text read out on every arrival is not a description, and a target meant to
/// be read says what it means as a name or a value.
fn relation_text<'a>(
    target: &'a Node,
    node: &impl Fn(NodeId) -> Option<&'a Node>,
) -> Option<String> {
    if let Some(text) = own_text(target) {
        return Some(text.to_owned());
    }
    let mut found: Vec<&str> = Vec::new();
    let mut stack: Vec<NodeId> = target.children().iter().rev().copied().collect();
    while let Some(id) = stack.pop() {
        let Some(n) = node(id) else { continue };
        if n.is_hidden() || n.role() == Role::TextRun {
            continue;
        }
        match own_text(n) {
            Some(text) => found.push(text),
            None => stack.extend(n.children().iter().rev().copied()),
        }
    }
    (!found.is_empty()).then(|| found.join(" "))
}
