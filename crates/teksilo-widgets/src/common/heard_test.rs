// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! What a screen reader is told when a widget tree changes, for tests.
//!
//! Reading a node's name after a key press says what a screen reader *could*
//! find if it looked. It says nothing about what it is *told*, and a user who
//! cannot see the screen hears only what they are told: a day that is right
//! in the tree and never announced is silence, and the same month announced
//! through two channels is heard twice. Both have shipped in the calendar
//! with every name-reading test green.
//!
//! A headless tree has no platform adapter, so this runs each accessibility
//! update through `accesskit_consumer`, the crate all three adapters are
//! built on, and keeps the change events they turn into speech. Each kind
//! below says which adapter code raises it, read from
//! `accesskit_atspi_common-0.20.0`, `accesskit_windows-0.35.0` and
//! `accesskit_macos-0.27.0`.
//!
//! Two events are raised and deliberately not kept: a name or value change
//! on a node that is not the focus. Orca speaks a name change only for its
//! locus of focus (`orca/scripts/default.py`, `onNameChanged`, Orca 46.1),
//! and AT-SPI carries no string value at all (`accesskit_atspi_common`
//! `node.rs:650-658` raises only a numeric one). That NVDA and VoiceOver
//! likewise speak a name or value change only on their focus object is not
//! verified here: neither's source is on this machine.
//!
//! Two things a reader can be told are not modelled, so a test about either
//! must not rest on this alone. A state change on the focus: Orca says
//! "selected" when Space selects its locus of focus (`onSelectedChanged`).
//! And the descendants of a node that stops being hidden: AT-SPI adds that
//! whole subtree in one walk and announces each live node in it
//! (`accesskit_atspi_common` `adapter.rs`, `add_subtree`), where the consumer
//! reports only the node whose own data changed.

use accesskit_consumer::{FilterResult, NodeRef, Tree, TreeChangeHandler, common_filter};
use teksilo_core::accesskit::Role;
use teksilo_core::widget_tree::WidgetTree;

/// One thing a screen reader is told.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Heard {
    /// Focus moved, and this is the name of the node that took it, after
    /// `active_descendant` is followed (`accesskit_consumer` `tree.rs:537-543`).
    /// AT-SPI raises `state-changed:focused` on it (`accesskit_atspi_common`
    /// `adapter.rs:324-340`), UIA a focus-changed event (`accesskit_windows`
    /// `adapter.rs:341-345`), macOS `FocusedUIElementChanged`
    /// (`accesskit_macos` `event.rs:319-326`). Every reader speaks it.
    Focus(String),
    /// A live region spoke this text. On all three platforms that is a live
    /// node entering the filtered tree with a name, or a live node's name
    /// changing (`accesskit_atspi_common` `adapter.rs:72-77`, `node.rs:610-622`;
    /// `accesskit_windows` `adapter.rs:256-263`, `314-324`; `accesskit_macos`
    /// `event.rs:236-241`, `300-310`). UIA and macOS also speak it when only
    /// the `live` setting changed, which is kept here too. The setting is
    /// inherited: `accesskit_consumer`'s `NodeRef::live` (`node.rs:906-910`)
    /// hands it to every descendant that sets none.
    Live(String),
    /// The node that holds focus was renamed. Orca speaks it as a change of
    /// its locus of focus (`onNameChanged`), whether or not the node is live,
    /// so a focused live node renamed is heard twice.
    FocusName(String),
    /// The node that holds focus took a new value. UIA and macOS raise it
    /// (`accesskit_windows` `node.rs:1346`, `accesskit_macos`
    /// `event.rs:262-289`); AT-SPI does not, for a string.
    FocusValue(String),
}

/// A screen reader attached to a tree, hearing what each change tells it.
pub(crate) struct Listener {
    platform: Tree,
}

impl Listener {
    /// Start listening to `tree` as it stands now. Nothing already in it is
    /// heard: a reader attaching to a window is not told what was said before.
    pub(crate) fn attach(tree: &mut WidgetTree) -> Self {
        let platform = Tree::new(tree.sync_accessibility(), true);
        let mut listener = Self { platform };
        // Let any message queued while the tree was being built go by
        // unheard: the framework's announcer puts one message in the tree an
        // update.
        let _ = listener.heard(tree);
        listener
    }

    /// Everything the reader is told between the last call and now.
    ///
    /// The tree is synced three times, as a running application syncs on
    /// the frames that follow a change: the framework's announcer puts one
    /// queued message in the tree an update, so a change that queues two
    /// needs two syncs to be heard whole, and the third shows that nothing is
    /// said again.
    pub(crate) fn heard(&mut self, tree: &mut WidgetTree) -> Vec<Heard> {
        let mut handler = Handler { heard: Vec::new() };
        for _ in 0..3 {
            let update = tree.sync_accessibility();
            self.platform
                .update_and_process_changes(update, &mut handler);
        }
        handler.heard
    }

    /// Whether the reader can find a node of `role` named `name` without
    /// being told about it, walking the tree as it stood at the last call as
    /// the adapters walk it (`common_filter`). A node that is published only
    /// once it holds focus fails this, and object navigation and flat review
    /// never reach it.
    pub(crate) fn finds(&self, role: Role, name: &str) -> bool {
        fn walk(node: NodeRef<'_>, role: Role, name: &str) -> bool {
            (node.role() == role && node.label().as_deref() == Some(name))
                || node
                    .filtered_children(&common_filter)
                    .any(|child| walk(child, role, name))
        }
        walk(self.platform.state().root(), role, name)
    }
}

struct Handler {
    heard: Vec<Heard>,
}

fn included(node: &NodeRef) -> bool {
    common_filter(node) == FilterResult::Include
}

impl TreeChangeHandler for Handler {
    fn node_added(&mut self, node: &NodeRef) {
        if included(node) && node.live() != teksilo_core::accesskit::Live::Off {
            if let Some(name) = node.label() {
                self.heard.push(Heard::Live(name));
            }
        }
    }

    fn node_updated(&mut self, old: &NodeRef, new: &NodeRef) {
        if !included(new) {
            return;
        }
        let name = new.label();
        if new.live() != teksilo_core::accesskit::Live::Off
            && let Some(name) = name.clone()
            && (!included(old) || old.label().as_ref() != Some(&name) || old.live() != new.live())
        {
            self.heard.push(Heard::Live(name));
        }
        if !included(old) {
            return;
        }
        if new.is_focused() {
            if let Some(name) = name
                && old.label().as_ref() != Some(&name)
            {
                self.heard.push(Heard::FocusName(name));
            }
            if let Some(value) = new.value()
                && old.value().as_ref() != Some(&value)
            {
                self.heard.push(Heard::FocusValue(value));
            }
        }
    }

    fn focus_moved(&mut self, _old: Option<&NodeRef>, new: Option<&NodeRef>) {
        if let Some(node) = new {
            self.heard
                .push(Heard::Focus(node.label().unwrap_or_default()));
        }
    }

    fn node_removed(&mut self, _node: &NodeRef) {}
}
